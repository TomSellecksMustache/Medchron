#!/usr/bin/env python3
"""OPTIONAL offline CUDA OCR adapter for MEDCHRON_OCR_EXECUTABLE.

Requires an operator-installed PaddlePaddle GPU runtime, PaddleOCR 3.x,
pre-downloaded detection/recognition models, and matching CUDA drivers.
No downloads are performed intentionally; do not use with clinical PII
without validating the dependency's networking behaviour and isolating it.
"""
import json
import os
import pathlib
import sys


def main() -> int:
    if len(sys.argv) != 2:
        print("Usage: medchron_paddle_cuda.py PAGE_IMAGE.png", file=sys.stderr)
        return 2
    image = pathlib.Path(sys.argv[1]).resolve(strict=True)
    detection = pathlib.Path(os.environ["MEDCHRON_PADDLE_DETECTION_DIR"]).resolve(strict=True)
    recognition = pathlib.Path(os.environ["MEDCHRON_PADDLE_RECOGNITION_DIR"]).resolve(strict=True)
    if not detection.is_dir() or not recognition.is_dir():
        raise RuntimeError("OCR model directories must exist locally")
    os.environ["HF_HUB_OFFLINE"] = "1"
    os.environ["TRANSFORMERS_OFFLINE"] = "1"
    from paddleocr import PaddleOCR  # operator installs and verifies offline

    ocr = PaddleOCR(
        text_detection_model_dir=str(detection),
        text_recognition_model_dir=str(recognition),
        use_doc_orientation_classify=False,
        use_doc_unwarping=False,
        use_textline_orientation=False,
        device=os.environ.get("MEDCHRON_PADDLE_DEVICE", "gpu:0"),
    )
    for page in ocr.predict(input=str(image)):
        data = page.json
        if isinstance(data, str):
            data = json.loads(data)
        contents = data.get("res", data)
        for item in contents.get("rec_texts", []):
            print(item)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as exc:
        print(f"Offline OCR adapter failed: {type(exc).__name__}", file=sys.stderr)
        raise SystemExit(1)
