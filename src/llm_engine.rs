// Purpose: LLM inference engine for QBO class prediction using MLX-LM server.
// Owner: Antigravity Agent
use crate::models::{LlmPrediction, TransactionLine};
use reqwest::Client;
use serde_json::{Value, json};

// MLX-LM local server defaults to 8080 usually via `mlx_lm.server`
const MLX_SERVER_URL: &str = "http://localhost:8080/v1/chat/completions";

/// Depth-aware JSON extractor: finds the first `{` and counts brace depth
/// to locate its matching `}`. Returns only the first top-level JSON object,
/// ignoring any trailing hallucinated output from the LLM.
fn extract_first_json_object(raw: &str) -> Option<&str> {
    let bytes = raw.as_bytes();
    let start = bytes.iter().position(|&b| b == b'{')?;
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut escape_next = false;

    for (i, &byte) in bytes[start..].iter().enumerate() {
        if escape_next {
            escape_next = false;
            continue;
        }
        match byte {
            b'\\' if in_string => {
                escape_next = true;
            }
            b'"' => {
                in_string = !in_string;
            }
            b'{' if !in_string => {
                depth += 1;
            }
            b'}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    return Some(&raw[start..start + i + 1]);
                }
            }
            _ => {}
        }
    }
    None // Unbalanced braces
}

/// Predict the QBO Class for a transaction line item.
/// `available_classes` is a list of (id, name) tuples representing the active QBO Classes.
pub async fn predict_class(
    tx: &TransactionLine,
    available_classes: &[(String, String)],
) -> Result<LlmPrediction, Box<dyn std::error::Error>> {
    let client = Client::new();

    // Build the class list for the prompt
    let class_list: String = available_classes
        .iter()
        .map(|(id, name)| format!("  - \"{}\" (ID: {})", name, id))
        .collect::<Vec<_>>()
        .join("\n");

    let system_prompt = format!(
        "You are an expert QBO accountant agent.\n\
         Your task is to classify a transaction line item into exactly ONE of the available QBO Classes listed below.\n\
         You MUST choose from this list — do NOT invent new classes or use account names.\n\n\
         Available QBO Classes:\n{}\n\n\
         You MUST output strictly in valid JSON matching this schema:\n\
         {{\n\
             \"class_ref\": \"<The exact Name of the class from the list above>\",\n\
             \"reasoning\": \"<A brief 1 sentence explanation>\",\n\
             \"confidence_score\": <A float between 0.0 and 1.0>\n\
         }}\n\
         Do not output any markdown formatting or extra text. Just the raw JSON object.",
        class_list
    );

    let user_prompt = format!(
        "Transaction Type: {}\nEntity Name: {:?}\nHeader Memo: {:?}\nLine Description: {:?}\nAccount: {:?}\nAmount: {:?}",
        tx.tx_type, tx.entity_name, tx.header_memo, tx.line_description, tx.account, tx.amount
    );

    let payload = json!({
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_prompt}
        ],
        "temperature": 0.0,
        "response_format": { "type": "json_object" }
    });

    let res = client.post(MLX_SERVER_URL).json(&payload).send().await?;

    if !res.status().is_success() {
        return Err(format!("MLX Server Error: {}", res.status()).into());
    }

    let completion: Value = res.json().await?;
    let content_str = completion["choices"][0]["message"]["content"]
        .as_str()
        .ok_or("Failed to parse MLX response content")?;

    // Phi-3 (and other local models) often wrap the JSON in ```json...``` tags
    // or hallucinate additional content after the first JSON block.
    // Use depth-aware extraction to isolate the first valid { ... } object.
    let json_payload = extract_first_json_object(content_str).unwrap_or(content_str); // fallback to raw string if no braces found

    match serde_json::from_str::<LlmPrediction>(json_payload) {
        Ok(prediction) => Ok(prediction),
        Err(e) => {
            tracing::error!(
                "Failed to parse LLM Output into LlmPrediction struct. Raw LLM Output: \n{}",
                content_str
            );
            Err(e.into())
        }
    }
}
