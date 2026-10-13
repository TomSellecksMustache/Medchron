# Medchron synthetic demonstration records

**All records in this folder are fictional test data. They do not describe real people, facilities, medical examinations, insurance claims or treatment.** Do not mistake them for clinical evidence, medical advice or material appropriate for legal proceedings.

## Included material

The `samples/` directory contains the project's original synthetic examples, plus **30 additional searchable PDF records** for a fictional person called **Jordan Test**. The additional PDFs cover an invented chronology from February 2024 to January 2025. They include urgent care, primary care, physiotherapy, diagnostic imaging, specialist consultations, occupational therapy, medication review, work-capacity notes and insurer correspondence.

Every added record bears the heading **SYNTHETIC RECORD** and **DEMONSTRATION ONLY - NOT A REAL PATIENT**. The `MANIFEST.csv` in the sample-update download lists the 30 new filenames, dates, document types, and SHA-256 hashes.

## Where to find the sample PDFs

- **Windows installer:** `C:\Program Files\Medchron\samples\`
- **Windows portable ZIP:** the `samples\` directory alongside `bin\` in the extracted package
- **Source checkout:** `samples/` in the Medchron repository

Open Medchron using the desktop shortcut, then use **Choose files** to browse to the appropriate sample directory. The browser's native file-picker start location is controlled by the browser and operating system; Medchron does not force the folder to open automatically.

If you would like a convenient copy outside `Program Files`, copy the synthetic PDFs to a folder such as `%USERPROFILE%\Documents\Medchron Samples`. This is optional; the release ZIP already contains them.

## Suggested demonstration workflow

1. Select several PDFs from `samples/` or select all of them for a longer timeline.
2. Process them locally in Medchron.
3. Review each candidate against its quoted source, verify the date, and edit/accept/reject entries.
4. Export the resulting chronology as CSV or Markdown.

**Important:** This prototype uses rules-based extraction and requires human review. Automatic categories, dates and summaries may be incomplete or incorrect. The inclusion of these records does not imply the software is appropriate for real client data. Do not put real medical files or client-identifying data in a public repository or release artifact.
