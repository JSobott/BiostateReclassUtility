# Privacy Policy
**Last updated: March 9, 2026**

ReclassUtility ("we," "us," or "our") is committed to protecting your privacy. This Privacy Policy explains how we collect, use, disclose, and safeguard your information when you use our application.

## 1. Overview: Local-First Privacy
ReclassUtility is designed with a "local-first" architecture. Most of your data, including synchronized QuickBooks Online transaction data and classification rules, is stored locally on your own machine in a SQLite database and the macOS Keychain.

## 2. Data Collection
- **QuickBooks Data:** We fetch transaction data (General Ledger, Transactions, Classes) via official Intuit APIs to enable the application's functionality. This data remains on your local machine.
- **Authentication Data:** OAuth 2.0 Access and Refresh tokens are stored securely in your macOS Keychain. We do not transmit these tokens to any server other than Intuit's official authorization servers.
- **Usage Logs:** The application generates local logs for debugging and auditing purposes (Audit History). These logs are stored locally and are not transmitted to us.

## 3. Use of Your Data
We use the data fetched from QuickBooks Online solely to:
- Display and group transactions for your review.
- Apply classification rules.
- Update transaction classes in your QuickBooks Online company via API.

## 4. Third-Party Services
The Application interacts with QuickBooks Online (Intuit Inc.). Your use of QuickBooks Online is subject to Intuit's own Privacy Policy and Terms of Service. We do not share your data with any other third parties.

## 5. Data Security
We leverage industry-standard security practices, including the use of the macOS Keychain for sensitive credentials and secure HTTPS communication for API requests to Intuit.

## 6. Changes to This Privacy Policy
We may update our Privacy Policy from time to time. We will notify you of any changes by posting the new Privacy Policy on this page.

## 7. Contact Us
If you have any questions or suggestions about our Privacy Policy, do not hesitate to contact us at [partnerships@biostate.ai](mailto:partnerships@biostate.ai).
