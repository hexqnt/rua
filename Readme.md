# RUA

Динамика перехода территории в Российско-Украинском конфликте

[![CI](https://github.com/hexqnt/rua/actions/workflows/ci.yml/badge.svg)](https://github.com/hexqnt/rua/actions/workflows/ci.yml)

## Запуск

В корневой директории проекта:

```sh
cargo run -- run
```

## HTML-страница

График сохраняется в `dist/index.html` как полноценная страница, собранная через `maud`.
Путь можно переопределить флагом `--output-html`.
Для отображения Plotly и флагов стран используется CDN (нужен интернет при открытии HTML).

### Режимы запуска

- `run`: полный режим — скачивает данные с `deepstatemap.live`, сохраняет в `dist/history.csv`,
  обучает модель по конфигу и строит HTML с прогнозом.
- `download`: скачивает данные и сохраняет CSV по указанному пути.
- `forecast`: обучает модель по конфигу и сохраняет CSV с прогнозом.
- `render`: строит HTML по историческому CSV и CSV с прогнозом (прогноз обязателен).
- `completions`: генерирует автодополнения для shell.

Примеры:

```sh
# Полный режим (скачивание + прогноз + HTML)
cargo run -- run

# Только загрузка CSV
cargo run -- download --output-csv dist/history.csv

# Прогноз в CSV
cargo run -- forecast --csv dist/history.csv --output-csv dist/forecast.csv

# HTML из CSV и прогноза
cargo run -- render --csv dist/history.csv --forecast-csv dist/forecast.csv --output-html dist/custom.html

# Автодополнения (stdout)
cargo run -- completions bash > /tmp/rua.bash

# Автодополнения в файл
cargo run -- completions zsh --output dist/rua.zsh
```

## Прогноз

По умолчанию используется модель `trend-filter` и горизонт 365 дней. Обучение берёт данные
с **2022-11-22** включительно.

```sh
# Прогноз с дефолтами (trend-filter)
cargo run -- forecast

# Явные пути
cargo run -- forecast \
  --csv dist/history.csv \
  --output-csv dist/forecast.csv \
  --horizon-days 365
```
