#!/usr/bin/env python3
"""生成 Tauri updater latest.json。"""
import argparse
import json
from datetime import datetime, timezone


def main():
    p = argparse.ArgumentParser(description="Generate Tauri updater latest.json")
    p.add_argument("--version", required=True)
    p.add_argument("--aarch64-sig", required=True, help="Path to aarch64 .sig file")
    p.add_argument("--x86-64-sig", required=True, help="Path to x86_64 .sig file")
    p.add_argument("--url-aarch64", required=True)
    p.add_argument("--url-x86-64", required=True)
    p.add_argument("--output", required=True)
    p.add_argument("--notes", default="", help="Release notes (changelog text)")
    args = p.parse_args()

    data = {
        "version": args.version,
        "notes": args.notes or f"v{args.version}",
        "pub_date": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": {
            "darwin-aarch64": {
                "signature": open(args.aarch64_sig).read(),
                "url": args.url_aarch64,
            },
            "darwin-x86_64": {
                "signature": open(args.x86_64_sig).read(),
                "url": args.url_x86_64,
            },
        },
    }

    with open(args.output, "w") as f:
        json.dump(data, f, ensure_ascii=False, indent=2)
    print(f"Generated {args.output}")


if __name__ == "__main__":
    main()
