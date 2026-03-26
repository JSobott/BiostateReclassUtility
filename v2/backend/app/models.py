from enum import StrEnum
from pydantic import BaseModel
from typing import Optional


class TransactionStatus(StrEnum):
    Pending = "Pending"
    Approved = "Approved"
    Rejected = "Rejected"
    Validated = "Validated"
    Failed = "Failed"
    Posted = "Posted"


# Valid forward transitions for status guard
STATUS_TRANSITIONS = {
    TransactionStatus.Approved: [TransactionStatus.Pending],
    TransactionStatus.Validated: [TransactionStatus.Approved],
    TransactionStatus.Posted: [TransactionStatus.Approved, TransactionStatus.Validated],
    TransactionStatus.Failed: [TransactionStatus.Approved, TransactionStatus.Validated],
    TransactionStatus.Rejected: [TransactionStatus.Pending],
    TransactionStatus.Pending: [TransactionStatus.Failed],  # reset only from Failed
}


class TransactionLine(BaseModel):
    tx_id: str
    line_id: str
    sync_token: str
    tx_type: str
    tx_date: Optional[str] = None
    entity_name: Optional[str] = None
    header_memo: Optional[str] = None
    line_description: Optional[str] = None
    account: Optional[str] = None
    account_type: Optional[str] = None
    po_ref: Optional[str] = None
    amount: Optional[float] = None
    suggested_class_ref: Optional[str] = None
    llm_reasoning: Optional[str] = None
    confidence_score: Optional[float] = None
    status: TransactionStatus = TransactionStatus.Pending


class TransactionBucket(BaseModel):
    account: str
    entity_name: str
    suggested_class_ref: str
    tx_count: int
    total_amount: float
    avg_confidence: float


class ClassificationRule(BaseModel):
    id: Optional[int] = None
    condition_field: str
    condition_value: str
    target_class: str


class LlmPrediction(BaseModel):
    class_ref: str
    reasoning: str
    confidence_score: float


class AuditLog(BaseModel):
    id: str
    batch_id: str
    request_json: str
    response_json: str
    status: str
    timestamp: str


# Request DTOs
class BatchApproveRequest(BaseModel):
    account: str
    entity_name: str
    current_class: str
    override_class: Optional[str] = None


class ResetPendingRequest(BaseModel):
    account: str
    entity_name: str
    current_class: str


class ApprovalRequest(BaseModel):
    tx_id: str
    line_id: str
    suggested_class_ref: str


class WritebackRequest(BaseModel):
    dry_run: bool


class SeedTokensRequest(BaseModel):
    access_token: str
    refresh_token: str
    realm_id: str


class SyncRequest(BaseModel):
    start_date: Optional[str] = None
    end_date: Optional[str] = None


class DeleteRuleRequest(BaseModel):
    id: int
