//! Рендер HTML-страницы с Plotly-графиком.

use chrono::{DateTime, Utc};
use maud::{DOCTYPE, PreEscaped, html};
use plotly::Plot;

use super::DownloadLinks;
use super::chart::ChartSummary;
use super::data::{UNFRIENDLY_COUNTRIES, US_STATES};

const PAGE_TITLE: &str = "Территория подконтрольная РФ с начала СВО";
const PAGE_SUBTITLE: &str = "Динамика занятых территорий по датам.";
const PAGE_DESCRIPTION: &str = "Интерактивная страница с графиком динамики контролируемой территории в российско-украинском конфликте: площадь, изменения и прогноз.";
const PAGE_KEYWORDS: &str = "российско-украинский конфликт, контроль территории, площадь, динамика, график, прогноз, статистика";
const SITE_URL: &str = "https://rua.hexq.ru/";
const SITE_NAME: &str = "RUA";
const FAVICON_DATA_URI: &str = "data:image/svg+xml,%3Csvg%20xmlns='http://www.w3.org/2000/svg'%20viewBox='0%200%2064%2064'%3E%3Crect%20width='64'%20height='64'%20rx='14'%20fill='%232464a6'/%3E%3Ctext%20x='32'%20y='41'%20font-size='28'%20text-anchor='middle'%20font-family='IBM%20Plex%20Sans,%20Arial,%20sans-serif'%20fill='white'%3ER%3C/text%3E%3C/svg%3E";
const GENERATED_AT_FORMAT: &str = "%Y-%m-%d %H:%M UTC";
const GOOGLE_FONTS_CSS: &str =
    "https://fonts.googleapis.com/css2?family=IBM+Plex+Sans:wght@400;500;600&display=swap";
const PLOTLY_CDN: &str = "https://cdn.plot.ly/plotly-2.35.2.min.js";
const GITHUB_REPO_URL: &str = "https://github.com/hexqnt/rua";
const GITHUB_REPO_TEXT: &str = "github.com/hexqnt/rua";
const FLAG_CDN_BASE: &str = "https://flagcdn.com/24x18/";
const UNIT_THOUSAND_KM2: &str = "тыс. км²";
const UNIT_KM2: &str = "км²";
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

#[allow(clippy::too_many_lines)]
pub(super) fn render_plot_page(
    plot: &Plot,
    summary: &ChartSummary,
    generated_at: DateTime<Utc>,
    download_links: &DownloadLinks,
) -> String {
    let plot_html = plot.to_inline_html(Some("area-plot"));
    let latest_area_sq_km = summary.latest_area_km2 * 1000.0;
    let forecast_area_sq_km = summary
        .forecast
        .as_ref()
        .map(|forecast| forecast.mean_km2 * 1000.0);
    let country_rows = build_country_rows(latest_area_sq_km, forecast_area_sq_km);
    let generated_label = generated_at.format(GENERATED_AT_FORMAT).to_string();
    let latest_area_label = format!("{:.1} {UNIT_THOUSAND_KM2}", summary.latest_area_km2);
    let ukraine_percent_label = format!("{:.2}%", summary.ukraine_percent);
    let daily_change_label = format_change(summary.daily_change_km2, UNIT_KM2);
    let weekly_change_label = format_change(summary.weekly_change_km2, UNIT_KM2);
    let history_download_label = format!("Скачать {}", download_links.history);
    let forecast_download_label = format!("Скачать {}", download_links.forecast);
    let forecast_card = summary.forecast.as_ref().map(|forecast| {
        (
            format!("Через {} дн.", forecast.horizon_days),
            format!("{:.1} {UNIT_THOUSAND_KM2}", forecast.mean_km2),
            format!(
                "95%: {:.1}–{:.1} {UNIT_THOUSAND_KM2} · до {}",
                forecast.lower_km2, forecast.upper_km2, forecast.end_date
            ),
        )
    });
    let page = html! {
        (DOCTYPE)
        html lang="ru" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                meta name="description" content=(PAGE_DESCRIPTION);
                meta name="keywords" content=(PAGE_KEYWORDS);
                link rel="canonical" href=(SITE_URL);
                link rel="icon" type="image/svg+xml" href=(FAVICON_DATA_URI);
                meta property="og:title" content=(PAGE_TITLE);
                meta property="og:description" content=(PAGE_DESCRIPTION);
                meta property="og:type" content="website";
                meta property="og:url" content=(SITE_URL);
                meta property="og:site_name" content=(SITE_NAME);
                meta name="twitter:card" content="summary";
                meta name="twitter:title" content=(PAGE_TITLE);
                meta name="twitter:description" content=(PAGE_DESCRIPTION);
                title { (PAGE_TITLE) }
                link rel="preconnect" href="https://fonts.googleapis.com";
                link rel="preconnect" href="https://fonts.gstatic.com" crossorigin;
                link rel="stylesheet"
                    href=(GOOGLE_FONTS_CSS);
                script src=(PLOTLY_CDN) {}
                style {
                    "
                    :root {
                        color-scheme: light;
                        --bg: #f7f6f2;
                        --card: #ffffff;
                        --ink: #1f2430;
                        --muted: #56606f;
                        --accent: #2464a6;
                        --border: rgba(31, 36, 48, 0.08);
                    }
                    * { box-sizing: border-box; }
                    body {
                        margin: 0;
                        background: var(--bg);
                        color: var(--ink);
                        font-family: \"IBM Plex Sans\", \"PT Sans\", sans-serif;
                    }
                    .page {
                        max-width: 1240px;
                        margin: 40px auto 60px;
                        padding: 0 24px;
                    }
                    .hero {
                        display: flex;
                        flex-wrap: wrap;
                        gap: 16px;
                        align-items: center;
                        justify-content: space-between;
                        margin-bottom: 22px;
                    }
                    .hero-aside {
                        display: flex;
                        flex-direction: column;
                        gap: 10px;
                        align-items: flex-end;
                    }
                    .title {
                        font-size: 26px;
                        font-weight: 600;
                        margin: 0;
                    }
                    .subtitle {
                        margin: 6px 0 0;
                        color: var(--muted);
                        font-size: 13px;
                    }
                    .link {
                        display: inline-flex;
                        align-items: center;
                        gap: 8px;
                        padding: 8px 14px;
                        border-radius: 999px;
                        border: 1px solid rgba(36, 100, 166, 0.25);
                        color: var(--accent);
                        font-weight: 500;
                        text-decoration: none;
                        transition: transform 0.2s ease, background 0.2s ease;
                    }
                    .link:hover {
                        transform: translateY(-1px);
                        background: rgba(36, 100, 166, 0.08);
                    }
                    .link svg {
                        width: 16px;
                        height: 16px;
                        display: block;
                    }
                    .flag {
                        width: 24px;
                        height: 18px;
                        margin-right: 8px;
                        border-radius: 2px;
                        box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.08);
                        object-fit: cover;
                    }
                    .card {
                        background: var(--card);
                        border-radius: 18px;
                        padding: 16px;
                        border: 1px solid var(--border);
                        overflow-x: auto;
                    }
                    .series-badges {
                        display: flex;
                        flex-wrap: wrap;
                        gap: 8px;
                        margin-bottom: 10px;
                    }
                    .badge {
                        display: inline-flex;
                        align-items: center;
                        gap: 8px;
                        padding: 4px 10px;
                        border-radius: 999px;
                        border: 1px solid rgba(36, 100, 166, 0.25);
                        background: rgba(36, 100, 166, 0.08);
                        color: var(--accent);
                        font-size: 11px;
                        font-weight: 600;
                        letter-spacing: 0.02em;
                    }
                    .badge::before {
                        content: "";
                        width: 8px;
                        height: 8px;
                        border-radius: 50%;
                        background: var(--accent);
                    }
                    .badge.forecast {
                        border-style: dashed;
                        background: rgba(36, 100, 166, 0.04);
                    }
                    .summary {
                        margin: 14px 0 18px;
                    }
                    .summary-grid {
                        display: grid;
                        grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
                        gap: 14px;
                    }
                    .summary-card {
                        background: var(--card);
                        border-radius: 16px;
                        padding: 14px 16px;
                        border: 1px solid var(--border);
                    }
                    .summary-label {
                        font-size: 11px;
                        text-transform: uppercase;
                        letter-spacing: 0.08em;
                        color: var(--muted);
                    }
                    .summary-label-icon {
                        display: inline-flex;
                        align-items: center;
                        justify-content: center;
                        margin-left: 6px;
                        color: var(--accent);
                        text-decoration: none;
                    }
                    .summary-label-icon:hover {
                        color: #1b4f87;
                    }
                    .summary-label-icon svg {
                        width: 12px;
                        height: 12px;
                    }
                    .summary-value {
                        font-size: 20px;
                        font-weight: 600;
                        margin-top: 6px;
                    }
                    .summary-sub {
                        margin-top: 6px;
                        font-size: 12px;
                        color: var(--muted);
                    }
                    .summary-link {
                        display: inline-flex;
                        margin-top: 8px;
                        font-size: 12px;
                        color: var(--accent);
                        text-decoration: none;
                        border-bottom: 1px dashed rgba(36, 100, 166, 0.45);
                    }
                    .table-card {
                        background: var(--card);
                        border-radius: 18px;
                        padding: 18px 20px;
                        border: 1px solid var(--border);
                        margin-top: 18px;
                    }
                    .table-grid {
                        display: grid;
                        grid-template-columns: repeat(2, minmax(0, 1fr));
                        gap: 16px;
                    }
                    .table-box h3 {
                        margin: 0 0 8px;
                        font-size: 13px;
                        font-weight: 600;
                    }
                    .table-controls {
                        display: flex;
                        align-items: center;
                        gap: 10px;
                        margin-bottom: 10px;
                        font-size: 12px;
                        color: var(--muted);
                    }
                    .table-controls select {
                        font: inherit;
                        padding: 6px 8px;
                        border-radius: 8px;
                        border: 1px solid var(--border);
                        background: #fff;
                        color: var(--ink);
                    }
                    .table-title {
                        margin: 0 0 10px;
                        font-size: 16px;
                        font-weight: 600;
                    }
                    .ratio-table {
                        width: 100%;
                        border-collapse: collapse;
                        font-size: 13px;
                    }
                    .ratio-table th,
                    .ratio-table td {
                        padding: 8px 10px;
                        border-bottom: 1px solid var(--border);
                        text-align: left;
                    }
                    .ratio-table th {
                        color: var(--muted);
                        font-weight: 500;
                        letter-spacing: 0.02em;
                        text-transform: uppercase;
                        font-size: 11px;
                    }
                    .ratio-table tbody tr:nth-child(even) {
                        background: rgba(31, 36, 48, 0.02);
                    }
                    .ratio-table td.ratio-forecast {
                        color: var(--muted);
                    }
                    .ratio-note {
                        margin-top: 10px;
                        font-size: 11px;
                        color: var(--muted);
                    }
                    #area-plot {
                        width: 100%;
                        min-height: 640px;
                    }
                    footer {
                        margin-top: 16px;
                        font-size: 12px;
                        color: var(--muted);
                        text-align: right;
                    }
                    footer a {
                        color: inherit;
                        text-decoration: none;
                        border-bottom: 1px dotted rgba(86, 96, 111, 0.6);
                    }
                    @media (max-width: 900px) {
                        .title { font-size: 22px; }
                        #area-plot { min-height: 560px; }
                        .table-grid { grid-template-columns: 1fr; }
                        .hero-aside { width: 100%; align-items: flex-start; }
                    }
                    "
                }
            }
            body {
                div class="page" {
                    header class="hero" {
                        div {
                            h1 class="title" { (PAGE_TITLE) }
                            p class="subtitle" {
                                (PAGE_SUBTITLE)
                            }
                        }
                        div class="hero-aside" {
                            a class="link" href=(GITHUB_REPO_URL) aria-label="GitHub репозиторий" {
                                svg viewBox="0 0 24 24" aria-hidden="true" focusable="false" {
                                    path fill="currentColor" d="M12 .5C5.65.5.5 5.8.5 12.3c0 5.2 3.4 9.6 8.1 11.1.6.1.8-.3.8-.6v-2.1c-3.3.7-4-1.6-4-1.6-.5-1.3-1.3-1.7-1.3-1.7-1.1-.8.1-.8.1-.8 1.2.1 1.9 1.3 1.9 1.3 1.1 1.9 2.9 1.3 3.6 1 .1-.8.4-1.3.7-1.6-2.7-.3-5.5-1.4-5.5-6 0-1.3.5-2.3 1.2-3.2-.1-.3-.5-1.5.1-3.1 0 0 1-.3 3.3 1.2 1-.3 2-.4 3-.4s2 .1 3 .4c2.3-1.5 3.3-1.2 3.3-1.2.6 1.6.2 2.8.1 3.1.8.9 1.2 2 1.2 3.2 0 4.6-2.8 5.6-5.5 5.9.4.4.8 1.1.8 2.2v3.3c0 .3.2.7.8.6 4.7-1.5 8.1-5.9 8.1-11.1C23.5 5.8 18.4.5 12 .5z" {}
                                }
                                (GITHUB_REPO_TEXT)
                            }
                        }
                    }
                    section class="summary" {
                        div class="summary-grid" {
                            div class="summary-card" {
                                div class="summary-label" { "Текущая площадь" }
                                div class="summary-value" { (latest_area_label) }
                                div class="summary-sub" {
                                    "Доля от Украины: " (ukraine_percent_label)
                                }
                            }
                            div class="summary-card" {
                                div class="summary-label" { "Изменения" }
                                div class="summary-value" { (daily_change_label) " за сутки" }
                                div class="summary-sub" { (weekly_change_label) " за 7 дней" }
                            }
                            div class="summary-card" {
                                div class="summary-label" {
                                    "Последний срез"
                                    a class="summary-label-icon"
                                        href=(&download_links.history)
                                        download
                                        aria-label=(&history_download_label)
                                        title=(&history_download_label) {
                                        svg viewBox="0 0 24 24" aria-hidden="true" focusable="false" {
                                            path
                                                fill="currentColor"
                                                d="M12 3v10.17l3.59-3.58L17 11l-5 5-5-5 1.41-1.41L11 13.17V3h1zm-7 14h14v2H5v-2z" {}
                                        }
                                    }
                                }
                                div class="summary-value" { (summary.latest_date) }
                                div class="summary-sub" { "Сгенерировано: " (generated_label) }
                            }
                            @if let Some((forecast_title, forecast_value, forecast_range)) = forecast_card {
                                div class="summary-card" {
                                    div class="summary-label" {
                                        "Прогноз"
                                        a class="summary-label-icon"
                                            href=(&download_links.forecast)
                                            download
                                            aria-label=(&forecast_download_label)
                                            title=(&forecast_download_label) {
                                            svg viewBox="0 0 24 24" aria-hidden="true" focusable="false" {
                                                path
                                                    fill="currentColor"
                                                    d="M12 3v10.17l3.59-3.58L17 11l-5 5-5-5 1.41-1.41L11 13.17V3h1zm-7 14h14v2H5v-2z" {}
                                            }
                                        }
                                    }
                                    div class="summary-value" { (forecast_value) }
                                    div class="summary-sub" { (forecast_title) " · " (forecast_range) }
                                }
                            } @else {
                                div class="summary-card" {
                                    div class="summary-label" { "Прогноз" }
                                    div class="summary-value" { "Нет данных" }
                                    div class="summary-sub" { "Запустите с командой forecast" }
                                }
                            }
                        }
                    }
                    div class="card" {
                        div class="series-badges" {
                            span class="badge actual" { "Факт" }
                            @if summary.forecast.is_some() {
                                span class="badge forecast" { "Прогноз" }
                            }
                        }
                        (PreEscaped(plot_html))
                    }
                    section class="table-card" {
                        h2 class="table-title" { "Соотношение к территориям недружественных стран" }
                        div class="table-controls" {
                            span { "Сортировка:" }
                            select id="ratio-sort" {
                                option value="ratio" selected { "по соотношению" }
                                option value="name" { "по имени" }
                            }
                        }
                        div class="table-grid" {
                            div class="table-box" {
                                h3 { "Страны" }
                                table class="ratio-table" data-table="countries" {
                                    thead {
                                        tr {
                                            th { "Страна" }
                                            th { "Соотношение" }
                                            th { "Прогноз" }
                                        }
                                    }
                                    tbody {
                                        @for row in &country_rows.countries {
                                            tr class="ratio-row" data-name=(row.name) data-ratio=(row.ratio_value) {
                                                td {
                                                    img
                                                        class="flag"
                                                        loading="lazy"
                                                        alt=(format!("Флаг {}", row.name))
                                                        src=(format!("{FLAG_CDN_BASE}{}.png", row.flag)) {}
                                                    (row.name)
                                                }
                                                td { (&row.ratio) }
                                                td class="ratio-forecast" { (&row.forecast_ratio) }
                                            }
                                        }
                                    }
                                }
                            }
                            div class="table-box" {
                                h3 { "США — штаты" }
                                table class="ratio-table" data-table="states" {
                                    thead {
                                        tr {
                                            th { "Штат" }
                                            th { "Соотношение" }
                                            th { "Прогноз" }
                                        }
                                    }
                                    tbody {
                                        @for row in &country_rows.states {
                                            tr class="ratio-row" data-name=(row.name) data-ratio=(row.ratio_value) {
                                                td {
                                                    img
                                                        class="flag"
                                                        loading="lazy"
                                                        alt=(format!("Флаг {}", row.name))
                                                        src=(format!("{FLAG_CDN_BASE}{}.png", row.flag)) {}
                                                    (row.name)
                                                }
                                                td { (&row.ratio) }
                                                td class="ratio-forecast" { (&row.forecast_ratio) }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        p class="ratio-note" {
                            "Соотношение рассчитано по последнему значению графика. "
                            "Прогноз — по среднему значению на конец горизонта."
                        }
                        script {
                            (PreEscaped(r"
                            (() => {
                                const select = document.getElementById('ratio-sort');
                                if (!select) return;
                                const tables = Array.from(document.querySelectorAll('.ratio-table'));
                                const sortTable = (table, key) => {
                                    const tbody = table.querySelector('tbody');
                                    if (!tbody) return;
                                    const rows = Array.from(tbody.querySelectorAll('tr.ratio-row'));
                                    rows.sort((a, b) => {
                                        if (key === 'name') {
                                            return a.dataset.name.localeCompare(b.dataset.name, 'ru');
                                        }
                                        return parseFloat(b.dataset.ratio) - parseFloat(a.dataset.ratio);
                                    });
                                    rows.forEach(row => tbody.appendChild(row));
                                };

                                const applySort = () => tables.forEach(table => sortTable(table, select.value));
                                applySort();
                                select.addEventListener('change', applySort);
                            })();
                            "))
                        }
                    }
                    footer {
                        "Версия: " (APP_VERSION) " · Сгенерировано: " (generated_label) " · RUA · Источник: "
                        a href="https://deepstatemap.live" { "deepstatemap.live" }
                    }
                }
            }
        }
    };
    page.into_string()
}

struct CountryRow {
    name: &'static str,
    flag: &'static str,
    ratio: String,
    ratio_value: f64,
    forecast_ratio: String,
}

struct TableRows {
    countries: Vec<CountryRow>,
    states: Vec<CountryRow>,
}

fn build_country_rows(latest_area_sq_km: f64, forecast_area_sq_km: Option<f64>) -> TableRows {
    let countries = UNFRIENDLY_COUNTRIES
        .iter()
        .map(|(name, area, flag)| {
            let ratio_value = latest_area_sq_km / *area;
            let forecast_ratio = forecast_area_sq_km
                .map(|forecast_area| forecast_area / *area)
                .map_or_else(|| "—".to_string(), |value| format!("{value:.2}x"));
            CountryRow {
                name,
                flag,
                ratio: format!("{ratio_value:.2}x"),
                ratio_value,
                forecast_ratio,
            }
        })
        .collect::<Vec<_>>();

    let states = US_STATES
        .iter()
        .map(|(name, area, flag)| {
            let ratio_value = latest_area_sq_km / *area;
            let forecast_ratio = forecast_area_sq_km
                .map(|forecast_area| forecast_area / *area)
                .map_or_else(|| "—".to_string(), |value| format!("{value:.2}x"));
            CountryRow {
                name,
                flag,
                ratio: format!("{ratio_value:.2}x"),
                ratio_value,
                forecast_ratio,
            }
        })
        .collect::<Vec<_>>();
    TableRows { countries, states }
}

fn format_change(value: Option<f64>, unit: &str) -> String {
    value.map_or_else(|| "—".to_string(), |val| format!("{val:+.0} {unit}"))
}
