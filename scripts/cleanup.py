#!/usr/bin/env python3
"""
清理 data/ 目录下运行时生成的附属文件。

用法:
  uv run python scripts/cleanup.py                        # 交互式菜单
  uv run python scripts/cleanup.py status                 # 查看待清理文件
  uv run python scripts/cleanup.py backup                 # 备份到 data/_backups/<timestamp>/
  uv run python scripts/cleanup.py delete                 # 删除运行时文件（自动恢复目录结构）
  uv run python scripts/cleanup.py backup-delete [--yes]  # 先备份再删除
  uv run python scripts/cleanup.py restore                # 从备份恢复（交互选择）
  uv run python scripts/cleanup.py restore --from 20260428_123456  # 从指定备份恢复
"""

import argparse
import shutil
import subprocess
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).parent.parent
DATA = ROOT / "data"
BACKUPS = DATA / "_backups"


def collect_targets() -> list[Path]:
    return sorted(
        p for p in DATA.rglob("*") if p.is_file() and not p.is_relative_to(BACKUPS)
    )


def human_size(total_bytes: int) -> str:
    for unit in ("B", "KB", "MB", "GB"):
        if total_bytes < 1024:
            return f"{total_bytes:.1f} {unit}"
        total_bytes //= 1024
    return f"{total_bytes:.1f} TB"


def print_status(targets: list[Path]) -> None:
    if not targets:
        print("没有找到需要清理的文件。")
        return

    total = sum(p.stat().st_size for p in targets)
    by_dir: dict[str, list[Path]] = {}
    for p in targets:
        rel = p.relative_to(DATA)
        key = str(rel.parts[0]) if len(rel.parts) > 1 else "(根目录)"
        by_dir.setdefault(key, []).append(p)

    print(f"共 {len(targets)} 个文件，合计 {human_size(total)}\n")
    for dir_name, files in sorted(by_dir.items()):
        dir_size = sum(f.stat().st_size for f in files)
        print(f"  {dir_name}/  ({len(files)} 个文件，{human_size(dir_size)})")
        for f in files[:5]:
            print(f"    {f.relative_to(DATA)}")
        if len(files) > 5:
            print(f"    ... 还有 {len(files) - 5} 个")


def do_backup(targets: list[Path], timestamp: str) -> None:
    dest_root = BACKUPS / timestamp
    print(f"备份到 data/_backups/{timestamp}/ ...")
    for src in targets:
        dst = dest_root / src.relative_to(DATA)
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(src, dst)
    total = sum(p.stat().st_size for p in targets)
    print(f"已备份 {len(targets)} 个文件（{human_size(total)}）")


def do_delete(targets: list[Path]) -> None:
    print(f"删除 {len(targets)} 个文件 ...")
    for p in targets:
        p.unlink()

    # 深度优先删除空目录（不含 data/ 本身）
    for d in sorted(DATA.rglob("*"), reverse=True):
        if d.is_dir() and d != DATA and not d.is_relative_to(BACKUPS):
            try:
                d.rmdir()
            except OSError:
                pass

    # 恢复 git 追踪的目录结构（.gitkeep 等）
    print("恢复目录结构 ...")
    result = subprocess.run(
        ["git", "restore", "data/"],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if result.returncode == 0:
        print("完成。")
    else:
        print(
            f"git restore 失败，请手动运行 `git restore data/`\n{result.stderr.strip()}"
        )


def confirm(prompt: str) -> bool:
    return input(f"{prompt} [y/N] ").strip().lower() in ("y", "yes")


def list_backups() -> list[Path]:
    if not BACKUPS.exists():
        return []
    return sorted(p for p in BACKUPS.iterdir() if p.is_dir())


def pick_backup() -> Path | None:
    backups = list_backups()
    if not backups:
        print("没有找到备份。")
        return None

    print("可用备份：\n")
    for i, b in enumerate(backups, 1):
        files = list(b.rglob("*"))
        file_count = sum(1 for f in files if f.is_file())
        total = sum(f.stat().st_size for f in files if f.is_file())
        print(f"  {i}. {b.name}  ({file_count} 个文件，{human_size(total)})")

    print()
    raw = input(f"请选择备份 [1-{len(backups)}，0 取消]: ").strip()
    if raw == "0" or not raw:
        return None
    try:
        idx = int(raw) - 1
        if 0 <= idx < len(backups):
            return backups[idx]
    except ValueError:
        pass
    print("无效选项。")
    return None


def do_restore(backup_dir: Path, targets: list[Path], *, skip_confirm: bool) -> None:
    # 如果当前有文件，询问是否先备份
    if targets:
        total = sum(p.stat().st_size for p in targets)
        print(
            f"当前 data/ 下有 {len(targets)} 个文件（{human_size(total)}），恢复后将被覆盖。"
        )
        if not skip_confirm and confirm("是否先备份当前内容？"):
            do_backup(targets, datetime.now().strftime("%Y%m%d_%H%M%S"))
            print()

    src_files = sorted(p for p in backup_dir.rglob("*") if p.is_file())
    print(f"从 {backup_dir.name} 恢复 {len(src_files)} 个文件 ...")
    for src in src_files:
        dst = DATA / src.relative_to(backup_dir)
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(src, dst)
    print("恢复完成。")


def interactive_menu(targets: list[Path]) -> None:
    print("data/ 目录清理工具\n")
    print_status(targets)
    print()
    print("请选择操作:")
    print("  1. status        — 查看待清理文件")
    print("  2. backup        — 备份到 data/_backups/<timestamp>/")
    print("  3. delete        — 删除运行时文件")
    print("  4. backup-delete — 先备份再删除")
    print("  5. restore       — 从备份恢复")
    print("  0. 退出\n")

    choice = input("请输入选项 [0-5]: ").strip()
    action_map = {
        "1": "status",
        "2": "backup",
        "3": "delete",
        "4": "backup-delete",
        "5": "restore",
        "0": None,
    }
    action = action_map.get(choice)

    if action is None or choice == "0":
        print("已退出。")
        return

    run_action(action, targets, skip_confirm=False, backup_name=None)


def run_action(
    action: str, targets: list[Path], *, skip_confirm: bool, backup_name: str | None
) -> None:
    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")

    if action == "status":
        print_status(targets)

    elif action == "backup":
        if not targets:
            print("没有找到需要备份的文件。")
            return
        if not skip_confirm and not confirm("确认备份？"):
            print("已取消。")
            return
        do_backup(targets, timestamp)

    elif action == "delete":
        if not targets:
            print("没有找到需要清理的文件，无需操作。")
            return
        if not skip_confirm and not confirm("确认删除？"):
            print("已取消。")
            return
        do_delete(targets)

    elif action == "backup-delete":
        if not targets:
            print("没有找到需要清理的文件，无需操作。")
            return
        if not skip_confirm and not confirm("确认先备份再删除？"):
            print("已取消。")
            return
        do_backup(targets, timestamp)
        do_delete(targets)

    elif action == "restore":
        if backup_name:
            backup_dir = BACKUPS / backup_name
            if not backup_dir.exists():
                print(f"备份不存在：{backup_name}")
                return
        else:
            backup_dir = pick_backup()
            if backup_dir is None:
                print("已取消。")
                return
        if not skip_confirm and not confirm(f"确认从 {backup_dir.name} 恢复？"):
            print("已取消。")
            return
        do_restore(backup_dir, targets, skip_confirm=skip_confirm)


def main() -> None:
    parser = argparse.ArgumentParser(
        description="清理 data/ 目录运行时文件",
        add_help=True,
    )
    parser.add_argument(
        "action",
        nargs="?",
        choices=["status", "backup", "delete", "backup-delete", "restore"],
        help="操作类型（省略时进入交互菜单）",
    )
    parser.add_argument("--yes", "-y", action="store_true", help="跳过确认提示")
    parser.add_argument(
        "--from", dest="backup_name", metavar="TIMESTAMP", help="restore 时指定备份名称"
    )
    args = parser.parse_args()

    targets = collect_targets()

    if args.action is None:
        interactive_menu(targets)
    else:
        if args.action not in ("status", "restore"):
            print_status(targets)
            print()
        run_action(
            args.action, targets, skip_confirm=args.yes, backup_name=args.backup_name
        )


if __name__ == "__main__":
    main()
