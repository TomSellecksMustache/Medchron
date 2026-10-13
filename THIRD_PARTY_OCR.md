# OCR distribution and third-party notices

The Medchron OCR backend runs the open-source projects **Tesseract OCR** and **Poppler** as local executables. They are not written by Medchron.

- Tesseract: https://github.com/tesseract-ocr/tesseract (Apache-2.0, plus separately licensed components and model data).
- Poppler: https://poppler.freedesktop.org/ (GPL-licensed components and other bundled dependencies; review actual binary distribution terms).
- Optional user-installed PaddleOCR: https://github.com/PaddlePaddle/PaddleOCR and PaddlePaddle, with model-specific licensing.

**Before distributing a Windows installer containing these binaries:** audit the exact packaged versions, DLLs, tessdata model licenses and copyright notices, bundle all required license files and fulfill the applicable source-offer/redistribution requirements. The workflow copies the installed binary directories as a convenience, but this is **not** a legal compliance determination.

Do not distribute any installer containing third-party OCR binaries until the license audit has been completed.
