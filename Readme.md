# RUA

Динамика перехода территории в Российско-Украинском конфликте

[![CI](https://github.com/hexqnt/rua/actions/workflows/ci.yml/badge.svg)](https://github.com/hexqnt/rua/actions/workflows/ci.yml)

## Запуск

RUA принимает только один runtime-параметр: путь к конфигу.

```sh
cargo run -- --config config.toml
```

## Формат `config.toml`

Конфиг строгий: неизвестные поля приводят к ошибке.

- `mode`: `run | download | forecast | render`.
- `archive_csv`: архивировать CSV в `.csv.gz` и удалять исходные `.csv`.
- `[run]`: полный режим (скачивание + прогноз + HTML).
- `[download]`: только скачивание CSV.
- `[forecast]`: обучение модели и сохранение прогноза в CSV.
- `[render]`: сборка HTML по историческому CSV и CSV прогноза.
- `[chart]`: даты начала отрисовки для визуальных серий.
  - `gray_zone_start`: старт «Серой зоны» на верхнем и нижнем графиках.
  - `avg_change_start`: старт линии «Ср. изменение» (красная линия и YoY-блок).
- `model` и `[trend_filter]`: параметры модели (встроены в общий конфиг).

Относительные пути из конфига резолвятся от текущей директории запуска.

### Дефолтный конфиг

В репозитории уже есть рабочий `config.toml` с дефолтами.

```sh
cargo run -- --config config.toml
```

## HTML-страница

По умолчанию HTML сохраняется в `dist/index.html`.
Для отображения Plotly и флагов стран используется CDN (нужен интернет при открытии HTML).

## Примеры конфигов

### Полный режим с архивированием CSV

```toml
mode = "run"
archive_csv = true
model = "trend-filter"

[run]
output_html = "dist/index.html"
output_history_csv = "dist/history.csv"
output_forecast_csv = "dist/forecast.csv"
horizon_days = 365
minify_html = true

[chart]
gray_zone_start = "2023-02-05"
avg_change_start = "2022-11-23"
```

### Только скачивание CSV

```toml
mode = "download"

[download]
output_csv = "dist/history.csv"
```

### Прогноз в CSV

```toml
mode = "forecast"
model = "trend-filter"

[forecast]
csv = "dist/history.csv"
output_csv = "dist/forecast.csv"
horizon_days = 365
```

### HTML из CSV и прогноза

```toml
mode = "render"
archive_csv = false

[render]
csv = "dist/history.csv"
forecast_csv = "dist/forecast.csv"
output_html = "dist/custom.html"
minify_html = false

[chart]
gray_zone_start = "2023-02-05"
avg_change_start = "2022-11-23"
```

## Прогноз

Подробности о модели: [Model.md](Model.md).

По умолчанию используется модель `trend-filter` и горизонт 365 дней. Обучение берёт данные
с **2022-11-22** включительно.
