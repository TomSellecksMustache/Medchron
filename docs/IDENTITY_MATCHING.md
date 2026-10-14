# Local document identity matching (Medchron RC2 follow-up)

This feature is intended for synthetic-data demonstrations. It changes only Medchron's **local** matching behavior; no client document, phone number, identifier, or OCR text is sent to cloud services.

## Matching order

1. An unambiguous explicitly labeled `Patient Name:`/`Patient:` name auto-associates and checks the **Name** field, provided there is no conflicting patient ID or existing profile.
2. A document without a name can associate automatically when its labeled **Patient ID / health-card number** uniquely identifies an existing person or a named document in the same upload. The **Patient ID** field is checked. If ambiguous, the file remains Unassigned.
3. If no usable name or patient ID exists, Medchron may show a **possible** match from a phone number found on an already identity-confirmed document belonging to a single person. The document stays **Unassigned**. Open its PDF, compare the number with the source, check the **Phone number** field, then click **Confirm** to move it to that person. Phone-only association cannot happen silently.
4. A phone number that appears in the local provider-number whitelist is classified as a **provider number**, automatically checked as a provider contact and **never** used to identify a patient. A whitelisted or explicitly provider-labeled phone cannot by itself confirm a document's identity. When both provider and patient phone numbers appear, the unambiguous patient phone is preferred.
5. Any conflicting or multiple matches remain **Unassigned** for manual resolution.

Only document identity fields may be auto-matched; chronology entries and clinical assertions **remain unverified** until human review. Auto-matched documents display an `Auto-matched` label to distinguish them from human-reviewed items. Users can uncheck any field and save; subsequent loads respect that decision. Existing saved documents retain their recorded verification state; automatic matching applies to newly imported documents so past manual decisions are not silently overwritten.

## Provider phone whitelist

Create the following plain UTF-8 file on the same machine:

- **Windows installer:** `%LOCALAPPDATA%\Medchron\data\provider-phones.txt`
- **Development default:** `<working directory>/data/provider-phones.txt` (when `MEDCHRON_DATA` is not set)
- **Custom file path:** set `MEDCHRON_PROVIDER_PHONES_FILE` to an absolute path before launching the server.

Enter one North American provider phone number per line, with no names or patient data. Formats like `9025550100`, `(902) 555-0100`, or `1-902-555-0100` work. Lines beginning with `#` and blank lines are ignored. If the file does not exist, the whitelist is empty. An unreadable or malformed existing file stops automatic intake rather than proceeding with an unreliable whitelist.

The whitelist is re-read whenever a document is imported or its phone match is validated; it does not need a server restart.

**Do not commit your live provider list or case data to GitHub.** The real list belongs under the ignored local data directory; this package provides only an empty example file.

## Known limitations

- Only explicitly labeled patient names and IDs are used, and only 10-digit North American phone numbers are compared.
- OCR errors, namesakes, shared household phones, and incomplete provider lists can cause incorrect suggestions. Uncertain matches require a person to intervene.
- Default storage is not encrypted at rest. Use synthetic records only until encryption, authentication, and security review are complete.
- No Outlook, Apple, Google, or other external service integration is enabled.
