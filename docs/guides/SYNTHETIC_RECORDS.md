# Synthetic demonstration records

The **30 bundled medical PDFs are entirely fictional**, containing five demonstration people (six PDFs each):

| Fictional person | Synthetic ID | Documents |
|---|---|---:|
| Jordan Test | DEMO-JT-101 | 6 |
| Taylor Example | DEMO-TE-202 | 6 |
| Morgan Sample | DEMO-MS-303 | 6 |
| Casey Demo | DEMO-CD-404 | 6 |
| Avery Fixture | DEMO-AF-505 | 6 |

Fifteen PDFs are **image-only scans** for OCR testing; fifteen have native text.
Each document contains a labeled `Patient Name:` and `Patient ID:` so the local automatic import can group the files. All thirty files are on the Windows installer allowlist under `samples/` and are identified in `SAMPLE_MANIFEST.csv`.

**Use these records for demonstration only.** Medchron is a prototype with no login system or enabled encryption at rest. Automatic identities and extracted chronology events are not verified medical findings. Documents missing or conflicting identity data are kept Unassigned rather than being attached to a possibly incorrect person.

Choose the PDFs in the app's single upload control (or drag them onto the drop zone); processing begins as soon as the files are selected. Batch limit: 30 PDFs, 25 MB each. OCR runs locally; no cloud upload is required.
