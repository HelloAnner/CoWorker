"""探索平台用：把当前生产实例的有效配置打包成 zip 快照。

打包内容与 `data/`、`.coworker/` 目录本身运行时会持续增长的文件
（chromadb、日志、技能资源、宫殿资源等）都原样纳入，不做子集筛选，
保证导入方拿到的是与生产完全一致的运行时状态。

产出的 bundle 含 `providers.json`/`LLM__*_API_KEY` 等密钥，属敏感产物，只应
在本机/内网传输，调用方需自行确保落盘目录不被提交到版本控制。
"""

from __future__ import annotations

import zipfile
from pathlib import Path

from coworker.core.config import Config, apply_admin_config_file
from coworker.core.model_config import apply_runtime_model_config_file

_DATA_DIR = Path("data")
_COWORKER_DIR = Path(".coworker")


def load_effective_config() -> Config:
    """构造与运行中实例等价的有效配置（env/.env 覆盖 + 运行期模型切换覆盖）。"""
    config = apply_admin_config_file(Config())
    apply_runtime_model_config_file(config.llm)
    return config


def build_config_bundle(config: Config, dest_path: Path) -> None:
    """把 `config` 对应的配置快照打包成 zip，流式写入 `dest_path`。

    逐文件调用 `ZipFile.write`，不会把 `data/` 整体先读进内存，避免长期运行、
    体积较大的实例（chromadb、日志）导致导出接口内存暴涨。
    """
    dest_path.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(dest_path, mode="w", compression=zipfile.ZIP_DEFLATED) as zf:
        zf.writestr("config.json", config.model_dump_json(indent=2))

        if _DATA_DIR.is_dir():
            for path in sorted(_DATA_DIR.rglob("*")):
                if path.is_file():
                    zf.write(path, arcname=str(path))

        if _COWORKER_DIR.is_dir():
            for path in sorted(_COWORKER_DIR.rglob("*")):
                if path.is_file():
                    zf.write(path, arcname=str(path))

        providers_path = Path(config.llm.providers_file or "providers.json")
        if providers_path.is_file():
            zf.write(providers_path, arcname=str(providers_path))
