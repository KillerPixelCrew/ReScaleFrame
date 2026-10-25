"""Replace only displayed FSR2 output with a named intermediate, retaining history."""
import argparse
import hashlib
import json
from pathlib import Path

RELATIVE = Path("sdk/include/FidelityFX/gpu/fsr2/ffx_fsr2_accumulate.h")
EXPECTED = "f9564cb2ca4de0d50dc852a01f1fac71c066a4be92a8bcc9a27aaac345d082c6"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("reference", type=Path)
    parser.add_argument("stage", choices=("current", "history", "rectified"))
    parser.add_argument("receipt", type=Path)
    args = parser.parse_args()
    original = (args.reference / RELATIVE).read_bytes()
    if hashlib.sha256(original).hexdigest() != EXPECTED:
        raise SystemExit("Pinned stock accumulation shader hash differs")
    text = original.decode("utf-8")
    edits = []

    def replace(before, after):
        nonlocal text
        if text.count(before) != 1:
            raise SystemExit("Expected exactly one diagnostic shader anchor")
        text = text.replace(before, after)
        edits.append({"before": before, "after": after})

    if args.stage == "history":
        replace("    FfxFloat32 fThisFrameReactiveFactor = ffxMax(params.fDilatedReactiveFactor, fTemporalReactiveFactor);",
                "    const FfxFloat32x3 rsfDiagnosticYCoCg = fHistoryColor;\n\n"
                "    FfxFloat32 fThisFrameReactiveFactor = ffxMax(params.fDilatedReactiveFactor, fTemporalReactiveFactor);")
    else:
        replace("    FfxFloat32x4 fUpsampledColorAndWeight = ComputeUpsampledColorAndWeight(params, clippingBox, fThisFrameReactiveFactor);",
                "    FfxFloat32x4 fUpsampledColorAndWeight = ComputeUpsampledColorAndWeight(params, clippingBox, fThisFrameReactiveFactor);\n"
                "    FfxFloat32x3 rsfDiagnosticYCoCg = fUpsampledColorAndWeight.xyz;")
    if args.stage == "rectified":
        replace("        RectifyHistory(params, clippingBox, fHistoryColor, fAccumulation, fLockContributionThisFrame, fThisFrameReactiveFactor, fLumaInstabilityFactor);",
                "        RectifyHistory(params, clippingBox, fHistoryColor, fAccumulation, fLockContributionThisFrame, fThisFrameReactiveFactor, fLumaInstabilityFactor);\n"
                "        rsfDiagnosticYCoCg = fHistoryColor;")
    replace("    WriteUpscaledOutput(iPxHrPos, fHistoryColor);",
            "    // Diagnostic output only. Normal internal color, weight, locks and history above remain intact.\n"
            "    WriteUpscaledOutput(iPxHrPos, UnprepareRgb(YCoCgToRGB(rsfDiagnosticYCoCg), Exposure()));")
    history_store = "    StoreInternalColorAndWeight(iPxHrPos, FfxFloat32x4(fHistoryColor, fTemporalReactiveFactor));"
    if text.count(history_store) != 1 or text.index(history_store) >= text.index("    WriteUpscaledOutput(iPxHrPos, UnprepareRgb("):
        raise SystemExit("Internal history must be stored normally before diagnostic output")
    target = args.source / RELATIVE
    target.write_text(text, encoding="utf-8", newline="\n")
    args.receipt.parent.mkdir(parents=True, exist_ok=True)
    args.receipt.write_text(json.dumps({
        "scope": "Diagnostic only, no production correction or image acceptance",
        "sdk_commit": "c6efa6bf7f2027b3ec94f28578bb5965eabb9e55",
        "stage": args.stage, "file": str(RELATIVE), "before_sha256": EXPECTED,
        "after_sha256": hashlib.sha256(target.read_bytes()).hexdigest(),
        "conversion": "UnprepareRgb(YCoCgToRGB(stage), Exposure()); signed RGB retained in RGBA16F",
        "history_store_unchanged": True, "edits": edits,
    }, indent=2) + "\n")


if __name__ == "__main__":
    main()
