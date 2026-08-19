"""结构化日志：数据走 stdout，日志走 stderr。"""

from __future__ import annotations

import logging
import sys


def setup_logger(
    name: str = "inspirex",
    *,
    verbose: bool = False,
    quiet: bool = False,
) -> logging.Logger:
    logger = logging.getLogger(name)
    if verbose:
        level = logging.DEBUG
    elif quiet:
        level = logging.WARNING
    else:
        level = logging.INFO
    logger.setLevel(level)
    if not logger.handlers:
        handler = logging.StreamHandler(sys.stderr)
        handler.setFormatter(
            logging.Formatter(
                "%(asctime)s [%(levelname)s] %(name)s: %(message)s",
                datefmt="%Y-%m-%d %H:%M:%S",
            )
        )
        logger.addHandler(handler)
        logger.propagate = False
    else:
        logger.setLevel(level)
        for handler in logger.handlers:
            handler.setLevel(level)
    return logger
