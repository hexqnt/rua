#!/usr/bin/env bash
set -euo pipefail

# Настройка прокси (если нужно только для cargo, лучше ограничить область)
export HTTPS_PROXY="socks5://127.0.0.1:1080"

# Сборка и запуск бинарника
cargo build --release && ./target/release/rua

# Активация окружения Python и запуск скрипта
source ~/venv/myds313/bin/activate
python draw.py
