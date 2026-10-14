# Medchron v0.2.8-rc.2 — changes

- Replaced embedded Base64 logos with original-brand-derived transparent local image assets, served by the Rust loopback server.
- Center pane defaults to a navigable 30-day calendar anchored on today (-15 through +14 days). Uploaded-document and chronology dates highlight their matching days; date filtering is local.
- Retained one file input. Before first import: large drop zone. After records exist: compact blue upload bar on every view.
- Profile and chronology tools move below the calendar and appear only for the selected person.
- Each PDF now has backward-compatible metadata in case.json, extracted locally: name, record date, phone, patient/health-card ID, and calendar dates.
- Right document rail displays an unconfirmed triangle per PDF. Preview popup has four independent on/off verification toggles, with an explicit save/confirm action.
- Unassigned PDFs can be dragged from the right document list onto an identified person in the left People sidebar; the server copies and reassigns the file and entries, leaves all verification pending, and then removes the source file after saving metadata.
- Existing 30 synthetic PDFs across five identities remain the only allowlisted installer samples.
- Version is 0.2.8-rc.2; Windows installer remains a required prerelease asset, stable latest is unchanged.

## Security and known limitations

Do not process real client data. The prototype has no authentication, encryption at rest, or fully transactional multi-case migration. OCR-derived identity and chronology remain unverified until separately reviewed. The currently configured calendar does not contact any external provider. Windows compilation, real PDF intake, and installer publishing must complete successfully on GitHub Actions before this RC is considered built.
