# RUA

[🇺🇸 English](./README.md) · [🇷🇺 Русский](./README.ru.md)

RUA загружает историю изменения контроля территорий из DeepStateMap, строит
трендовый прогноз и формирует интерактивный HTML-отчёт.

[Открыть опубликованный отчёт](https://rua.hexq.ru)

[![CI](https://github.com/hexqnt/rua/actions/workflows/ci.yml/badge.svg)](https://github.com/hexqnt/rua/actions/workflows/ci.yml)
[![Deploy to Cloudflare Pages](https://github.com/hexqnt/rua/actions/workflows/deploy.yml/badge.svg)](https://github.com/hexqnt/rua/actions/workflows/deploy.yml)

## Установка и запуск

RUA — консольная программа с одним обязательным параметром: путём к файлу
конфигурации.

### Готовые бинарники

Скачайте архив для своей системы из
[последнего релиза на GitHub](https://github.com/hexqnt/rua/releases/latest):

| Система | Архив |
| --- | --- |
| Linux x86_64 (большинство дистрибутивов) | [`rua-linux-x86_64-gnu.tar.gz`](https://github.com/hexqnt/rua/releases/latest/download/rua-linux-x86_64-gnu.tar.gz) |
| Linux x86_64 (musl, включая Alpine) | [`rua-linux-x86_64-musl.tar.gz`](https://github.com/hexqnt/rua/releases/latest/download/rua-linux-x86_64-musl.tar.gz) |
| Windows x86_64 | [`rua-windows-x86_64-msvc.zip`](https://github.com/hexqnt/rua/releases/latest/download/rua-windows-x86_64-msvc.zip) |
| macOS на Apple Silicon | [`rua-macos-aarch64.tar.gz`](https://github.com/hexqnt/rua/releases/latest/download/rua-macos-aarch64.tar.gz) |

В архиве находится только исполняемый файл. Отдельно скачайте пример
[`config.toml`](./config.toml), поместите его в рабочую директорию и распакуйте
туда архив.

В Linux или macOS выполните:

```sh
./rua --config config.toml
```

В Windows PowerShell выполните:

```powershell
.\rua.exe --config .\config.toml
```

### Установка из исходников

Установите последнюю версию с GitHub при помощи стабильной версии Rust:

```sh
cargo install --git https://github.com/hexqnt/rua --locked
rua --config config.toml
```

Cargo устанавливает `rua` в свою директорию с бинарниками, которая должна быть
добавлена в `PATH`. Конфигурация автоматически не устанавливается — скачайте
[`config.toml`](./config.toml) отдельно.

## Конфигурация

В репозитории есть готовый к использованию [`config.toml`](./config.toml). Поле
`mode` определяет режим работы:

| Режим      | Результат                                                      |
| ---------- | -------------------------------------------------------------- |
| `run`      | Загрузить историю, построить прогноз и сформировать HTML-отчёт |
| `download` | Только загрузить историю в CSV                                 |
| `forecast` | Прочитать CSV с историей и сохранить прогноз в CSV             |
| `render`   | Сформировать HTML-отчёт из CSV с историей и прогнозом          |

Относительные пути в конфигурации отсчитываются от директории, из которой
запущена RUA. При использовании примера полный отчёт сохраняется в
`dist/index.html`. Страница загружает Plotly и флаги стран из CDN, поэтому при её
открытии требуется интернет-соединение.

Установите `archive_csv = true`, чтобы сжимать созданные CSV-файлы в `.csv.gz`.
Параметры графиков, прогноза и выходных файлов настраиваются в соответствующих
секциях конфигурации.

## Использование как библиотеки

Модуль `rua::deepstatemap` предоставляет типизированный асинхронный клиент для
загрузки истории DeepStateMap. Чтобы использовать его без зависимостей CLI и
генерации отчётов:

```toml
[dependencies]
rua = { git = "https://github.com/hexqnt/rua", default-features = false }
```

Техническое описание модели прогноза находится в [`Model.md`](./Model.md).

## Лицензия

Проект распространяется на выбор по условиям
[Apache License 2.0](./LICENSE-APACHE) или [MIT License](./LICENSE-MIT).
