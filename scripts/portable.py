#!/usr/bin/env python3
"""Pack the Windows portable ZIP: unzip-and-run, no installer required.

Output: src-tauri/target/release/bundle/portable/CuiLianTu_<ver>_x64_portable.zip

Zip layout (top-level folder so extraction never scatters files):
  CuiLianTu/CuiLianTu.exe      (renamed from target/release/cuiliantu.exe)
  CuiLianTu/使用说明.txt

Version is read from src-tauri/tauri.conf.json — the single source of truth.
Only the Python standard library is used (no third-party dependencies).

Usage:
  python scripts/portable.py            # pack from an existing release build
  python scripts/portable.py --check    # verify the existing zip instead
"""
from __future__ import annotations

import json
import sys
import zipfile
from datetime import date
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CONF = ROOT / "src-tauri" / "tauri.conf.json"
EXE = ROOT / "src-tauri" / "target" / "release" / "cuiliantu.exe"
OUT_DIR = ROOT / "src-tauri" / "target" / "release" / "bundle" / "portable"

README_NAME = "使用说明.txt"
README_TEMPLATE = """CuiLianTu（淬炼图）v{version} — Windows 便携版

【使用方法】
1. 把整个 CuiLianTu 文件夹解压到任意位置（不要只解压单个文件）
2. 双击文件夹里的 CuiLianTu.exe 即可运行——无需安装、无需管理员权限

【系统要求】
- Windows 10 / 11（64 位）
- WebView2 运行时：Windows 11 自带；部分 Windows 10 机器可能没有，
  若启动提示缺少 WebView2，请到
  https://developer.microsoft.com/microsoft-edge/webview2/
  下载「Evergreen Standalone Installer」安装一次即可

【与安装版的区别】
- 便携版不写注册表、不建卸载项：删除 CuiLianTu 文件夹即完成卸载
- 设置数据保存在 %APPDATA%\\app.cuiliantu.desktop，与安装版互不冲突

【其他】
- 所有图片处理均在本地完成，不会上传任何文件
- 命令行批量处理：可在终端运行 CuiLianTu.exe --help 查看说明
- 官网与更新：https://github.com/CYXue/cuiliantu

打包日期：{date}
"""


def version() -> str:
    return json.loads(CONF.read_text(encoding="utf-8"))["version"]


def readme_text(ver: str) -> str:
    return README_TEMPLATE.format(version=ver, date=date.today().isoformat())


def pack() -> Path:
    if sys.platform != "win32":
        sys.exit("[portable] 仅支持在 Windows 上打包（exe 是 Windows 产物）")
    if not EXE.is_file():
        sys.exit(f"[portable] 找不到 {EXE}——请先运行 pnpm tauri build")

    ver = version()
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    out = OUT_DIR / f"CuiLianTu_{ver}_x64_portable.zip"
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as zf:
        zf.write(EXE, "CuiLianTu/CuiLianTu.exe")
        zf.writestr(f"CuiLianTu/{README_NAME}", readme_text(ver), compress_type=zipfile.ZIP_DEFLATED)
    print(f"[portable] 打包完成: {out}")
    print(f"[portable] 大小: {out.stat().st_size / 1024 / 1024:.2f} MiB")
    return out


def check(out: Path | None = None) -> None:
    ver = version()
    if out is None:
        out = OUT_DIR / f"CuiLianTu_{ver}_x64_portable.zip"
    if not out.is_file():
        sys.exit(f"[portable] 找不到 {out}")
    with zipfile.ZipFile(out) as zf:
        bad = zf.testzip()
        if bad is not None:
            sys.exit(f"[portable] zip 损坏: {bad}")
        names = zf.namelist()
        expected = {f"CuiLianTu/CuiLianTu.exe", f"CuiLianTu/{README_NAME}"}
        if set(names) != expected:
            sys.exit(f"[portable] 内容不符: {names}")
        exe_bytes = zf.read("CuiLianTu/CuiLianTu.exe")
        if exe_bytes != EXE.read_bytes():
            sys.exit("[portable] zip 内 exe 与 target/release 不一致")
        txt = zf.read(f"CuiLianTu/{README_NAME}").decode("utf-8")
        if f"v{ver}" not in txt:
            sys.exit(f"[portable] 说明文件版本号与 tauri.conf.json ({ver}) 不一致")
    print(f"[portable] 校验通过: {out} ({out.stat().st_size / 1024 / 1024:.2f} MiB)")


if __name__ == "__main__":
    if "--check" in sys.argv:
        check()
    else:
        check(pack())
