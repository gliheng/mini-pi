#!/usr/bin/env python3
"""Generate Windows .ico and macOS .icns icons from assets/icons/pi.svg."""

from pathlib import Path

import cairosvg

ROOT = Path(__file__).resolve().parent.parent
SVG_PATH = ROOT / "assets" / "icons" / "pi.svg"
ICO_PATH = ROOT / "scripts" / "installer" / "app.ico"
ICNS_PATH = ROOT / "scripts" / "installer" / "app.icns"

ICO_SIZES = [16, 32, 48, 64, 128, 256]
ICNS_SIZES = [16, 32, 64, 128, 256, 512, 1024]

# Apple ICNS type codes for PNG-encoded images.
ICNS_TYPES = {
    16: b"icp4",
    32: b"icp5",
    64: b"icp6",
    128: b"ic07",
    256: b"ic08",
    512: b"ic09",
    1024: b"ic10",
}


def render_png(svg: bytes, size: int) -> bytes:
    return cairosvg.svg2png(bytestring=svg, output_width=size, output_height=size)


def write_ico(svg: bytes, sizes: list[int], out: Path) -> None:
    # Render each size as a PNG and assemble an ICO with PNG entries.
    pngs = [render_png(svg, size) for size in sizes]
    count = len(sizes)

    # ICO header: Reserved (2), Type (2), Count (2)
    header = (0).to_bytes(2, "little") + (1).to_bytes(2, "little") + count.to_bytes(2, "little")

    # ICONDIRENTRY is 16 bytes each. Data starts after header + entries.
    entry_size = 16
    data_offset = len(header) + count * entry_size
    entries = bytearray()
    data = bytearray()

    for size, png in zip(sizes, pngs):
        width = size if size < 256 else 0
        height = width
        entry = bytes([
            width, height,  # bWidth, bHeight
            0,              # bColorCount
            0,              # bReserved
        ])
        entry += (1).to_bytes(2, "little")       # wPlanes
        entry += (32).to_bytes(2, "little")      # wBitCount
        entry += len(png).to_bytes(4, "little")  # dwBytesInRes
        entry += data_offset.to_bytes(4, "little")  # dwImageOffset
        entries += entry
        data += png
        data_offset += len(png)

    out.write_bytes(header + entries + data)
    print(f"wrote {out}")


def write_icns(svg: bytes, sizes: list[int], out: Path) -> None:
    chunks = bytearray()
    for size in sizes:
        png = render_png(svg, size)
        type_code = ICNS_TYPES[size]
        chunk_len = 8 + len(png)
        chunks += type_code
        chunks += chunk_len.to_bytes(4, "big")
        chunks += png

    # ICNS file header: magic + total file length.
    file_len = 8 + len(chunks)
    out.write_bytes(b"icns" + file_len.to_bytes(4, "big") + chunks)
    print(f"wrote {out}")


def main() -> None:
    svg = SVG_PATH.read_bytes()
    write_ico(svg, ICO_SIZES, ICO_PATH)
    write_icns(svg, ICNS_SIZES, ICNS_PATH)


if __name__ == "__main__":
    main()
