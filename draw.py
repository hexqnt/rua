"""
Отрисовка динамики изменения территорий в российско-украинском конфликте
"""

import datetime
import sys
from pathlib import Path
from typing import Optional, Tuple

import matplotlib.pyplot as plt
import numpy as np
import pandas as pd
import seaborn as sns
from statsmodels.tsa.arima.model import ARIMA, ARIMAResults
from statsmodels.tsa.tsatools import add_trend

# pylint: disable=invalid-name

DATA_PATH = Path("data/area_history.csv")
IMAGE_PATH = Path("img/area.png")
SVO_PREFIX_DATE = "2022-11-11"

FORECAST_HORIZON_DAYS = 120
CONFIDENCE_ALPHA = 0.01
BOUND_FACTOR = 0.995
MAX_PQ = 6
MOMENTUM_YEAR_DAYS = 365
MOMENTUM_MONTH_DAYS = 31
ANNOTATION_SHIFT_DAYS = 19
ROLLING_WINDOW = 5
ROLLING_MIN_PERIODS = 3
MAX_SEARCH_OFFSET_DAYS = 60

DTYPES = {
    "percent": "float32",
    "area": "float64",
    "hash": "string",
    "area_type": "category",
}


def add_segment_trend(
    features: pd.DataFrame, start_date: pd.Timestamp, column: str
) -> pd.DataFrame:
    """Добавляет линейный тренд, стартующий с нуля в указанную дату."""
    if not (features.index.min() <= start_date <= features.index.max()):
        msg = f"Date {start_date} is out of range [{features.index.min()}, {features.index.max()}]"
        raise ValueError(msg)

    updated = features.copy()
    updated[column] = 0
    updated.loc[start_date:, column] = np.arange(len(updated.loc[start_date:, column]))
    return updated


def load_area_history(path: Path = DATA_PATH) -> pd.DataFrame:
    """Загружает исходный CSV с едиными правилами парсинга."""
    return pd.read_csv(
        path,
        index_col="time_index",
        parse_dates=True,
        dtype=DTYPES,
    )


def build_daily_area(df: pd.DataFrame) -> pd.DataFrame:
    """Агрегирует данные по дням с сохранением типов территорий."""
    return (
        df.dropna()
        .groupby([pd.Grouper(freq="D"), "area_type"], observed=False)[["area", "percent"]]
        .mean()
        .reset_index()
        .set_index("time_index")
    )


def compute_occupied_by_ua(df: pd.DataFrame) -> pd.DataFrame:
    """Среднесуточные площадь и доля, контролируемые Украиной."""
    return (
        df[(df["area_type"] == "other_territories") & (df["hash"] == "#01579b")]
        .groupby(pd.Grouper(freq="D"))[["area", "percent"]]
        .mean()
        .interpolate()
    )


def build_occupied_by_ru(
    area_dynamic: pd.DataFrame, occupied_by_ua: pd.DataFrame
) -> pd.DataFrame:
    """Ряд суточных значений площади под контролем РФ с учётом продвижения Украины."""
    ru_area = area_dynamic[area_dynamic["area_type"] == "occupied_after_24_02_2022"][
        "area"
    ].copy()
    prefix_date = pd.Timestamp(SVO_PREFIX_DATE)
    if prefix_date.tzinfo is None and ru_area.index.tz is not None:
        prefix_date = prefix_date.tz_localize(ru_area.index.tz)

    full_range = pd.DataFrame(
        index=pd.date_range(
            start=ru_area.index.min(), end=ru_area.index.max(), freq="D"
        )
    )
    ru_area = full_range.join(ru_area).interpolate()
    ru_area["prefix"] = 0
    ru_area.loc[:prefix_date, "prefix"] = 1
    ru_area["area"] = ru_area["area"].subtract(occupied_by_ua["area"], fill_value=0)
    return ru_area


def build_features(y: pd.Series, horizon_days: int) -> pd.DataFrame:
    """Формирует матрицу экзогенных признаков с трендом и моментумами."""
    index = pd.date_range(
        y.index.min(), y.index.max() + pd.DateOffset(days=horizon_days), freq="D"
    )
    features = add_trend(pd.DataFrame(index=index), "ct")
    features = add_segment_trend(
        features, y.index.max() - pd.DateOffset(days=MOMENTUM_YEAR_DAYS), "momentum_y"
    )
    features = add_segment_trend(
        features, y.index.max() - pd.DateOffset(days=MOMENTUM_MONTH_DAYS), "momentum_m"
    )
    return features


def select_arima_model(y: pd.Series, features: pd.DataFrame) -> ARIMAResults:
    """Выбирает лучшую ARIMA(p,0,q) по AIC, отдавая приоритет простым моделям."""
    pq = [(p, q) for p in range(MAX_PQ) for q in range(MAX_PQ)]
    pq.sort(key=lambda pair: pair[0] + pair[1])

    best_model: Optional[ARIMAResults] = None
    for p, q in pq:
        model = ARIMA(y, exog=features.loc[: y.index.max()], order=(p, 0, q), trend="n")
        mod_fit = model.fit()
        if best_model is None or (best_model.aic * BOUND_FACTOR) > mod_fit.aic:
            best_model = mod_fit

    if best_model is None:
        raise RuntimeError("Failed to fit ARIMA model")
    return best_model


def forecast_area(
    model: ARIMAResults, y: pd.Series, features: pd.DataFrame, horizon_days: int
) -> pd.DataFrame:
    """Строит прогноз и выравнивает полученный индекс дат."""
    forecast = model.get_forecast(
        horizon_days,
        alpha=CONFIDENCE_ALPHA,
        exog=features.loc[y.index.max() + pd.DateOffset(days=1) :],
    ).summary_frame()
    forecast.index = pd.date_range(
        start=y.index.max() + pd.DateOffset(days=1),
        periods=forecast.shape[0],
        freq="D",
    )
    return forecast


def trim_forecast(fcst: pd.DataFrame, alpha_threshold: float, km_ratio: float) -> pd.DataFrame:
    """Обрезает прогноз, когда выполнены пороги доверия и темпа изменения."""
    change_ratio = fcst[["mean", "mean_se"]].diff() / fcst[["mean", "mean_se"]].abs()
    end_svo = change_ratio[
        (change_ratio["mean_se"] <= 1 - alpha_threshold / 100)
        & (change_ratio["mean"] <= km_ratio / 100)
    ].index.min()
    if pd.isna(end_svo):
        return fcst
    return fcst.loc[:end_svo]


def compute_daily_change(occupied_by_ru: pd.DataFrame) -> pd.DataFrame:
    """Сглаженные посуточные изменения контролируемой площади."""
    return (
        occupied_by_ru.diff().loc["2022-11-23":]
        .rolling(ROLLING_WINDOW, center=True, min_periods=ROLLING_MIN_PERIODS)
        .mean()
    )


def summarize_recent_changes(day_change: pd.DataFrame) -> Tuple[float, float]:
    """Возвращает недельный и месячный итог изменения площади."""
    week = float(day_change["area"][-7:].sum())
    month = float(day_change["area"][-30:].sum())
    return week, month


def plot_occupied_area(ax, occupied_by_ru: pd.DataFrame, fcst: pd.DataFrame) -> None:
    """Рисует фактическую и прогнозную площадь с подписью ключевых точек."""
    max_window = occupied_by_ru.iloc[
        -365 - MAX_SEARCH_OFFSET_DAYS : -MAX_SEARCH_OFFSET_DAYS
    ]["area"]
    max_idx = max_window.idxmax()
    max_val = round(max_window.max() / 1000, 1)

    sns.lineplot(occupied_by_ru["area"] / 1000, ax=ax, label="Факт")
    sns.lineplot(fcst["mean"] / 1000, ls="--", ax=ax, label="Ожидание")
    fill_95p = ax.fill_between(
        fcst.index,
        fcst["mean_ci_lower"] / 1000,
        fcst["mean_ci_upper"] / 1000,
        alpha=0.2,
        color="grey",
    )
    fill_95p.set_label("99% дов. интервал")
    ax.legend()
    ax.set(
        xlabel=None,
        ylabel="тыс. км\u00b2",
        title="Территория подконтрольная РФ с начала СВО",
    )

    ax.text(max_idx, max_val * 1.01, f"{max_val:.1f}", ha="center", va="bottom")

    ax.text(
        occupied_by_ru.index.max(),
        occupied_by_ru["area"].iloc[-1] / 1000 * 1.01,
        f"{occupied_by_ru['area'].iloc[-1] / 1000:.1f}",
        ha="center",
        va="bottom",
    )

    ax.text(
        fcst.index.max() + pd.DateOffset(days=3),
        fcst["mean"].iloc[-1] / 1000,
        f"{fcst['mean'].iloc[-1] / 1000:.1f}",
        ha="left",
        va="center",
        color="darkorange",
    )

    ax.text(
        fcst.index.max(),
        fcst["mean_ci_upper"].iloc[-1] / 1000 * 1.01,
        f"{fcst['mean_ci_upper'].iloc[-1] / 1000:.1f}",
        ha="center",
        va="bottom",
        color="grey",
    )

    ax.text(
        fcst.index.max(),
        fcst["mean_ci_lower"].iloc[-1] / 1000 * 0.99,
        f"{fcst['mean_ci_lower'].iloc[-1] / 1000:.1f}",
        ha="center",
        va="top",
        color="grey",
    )


def plot_daily_change(ax, day_change: pd.DataFrame) -> None:
    """Рисует сглаженные суточные изменения с подписью на конце ряда."""
    sns.lineplot(day_change["area"], ax=ax, legend=None)
    ax.fill_between(
        day_change.index,
        0,
        day_change["area"],
        color="royalblue",
        alpha=0.1,
    )
    bbox = {"boxstyle": "larrow", "fc": "0.8", "alpha": 0.4}
    dy = float(day_change.iloc[-1]["area"])
    dx = day_change.index.max()
    ax.annotate(
        f"{dy:.2f}",
        (dx + datetime.timedelta(days=ANNOTATION_SHIFT_DAYS), dy),
        bbox=bbox,
        va="center",
        ha="left",
    )
    ax.set(
        xlabel=None,
        ylabel="км\u00b2/сутки",
        title="Среднесуточное изменение",
    )


def plot_layout(
    occupied_by_ru: pd.DataFrame,
    fcst: pd.DataFrame,
    last_date: str,
    day_change: pd.DataFrame,
) -> None:
    """Создаёт финальный макет графиков и сохраняет изображение."""
    fig, axs = plt.subplots(2, 1, sharex=True, figsize=(12, 6))

    plot_occupied_area(axs[0], occupied_by_ru, fcst)
    plot_daily_change(axs[1], day_change)

    for ax in axs:
        ax.grid(ls=":", lw=0.5)

    fig.tight_layout()
    fig.text(
        0,
        0,
        f"Источник: DeepStateMap от {last_date}",
        fontdict={"size": 8},
        alpha=0.45,
    )
    IMAGE_PATH.parent.mkdir(exist_ok=True)
    fig.savefig(IMAGE_PATH, format="png", dpi=300)


def main() -> int:
    """Точка входа скрипта."""
    df = load_area_history()
    last_date = df.index.max().strftime("%Y-%m-%d %X")
    area_dynamic = build_daily_area(df)
    occupied_by_ua = compute_occupied_by_ua(df)
    occupied_by_ru = build_occupied_by_ru(area_dynamic, occupied_by_ua)

    y = occupied_by_ru.loc["2022-11-12":, "area"]
    features = build_features(y, FORECAST_HORIZON_DAYS)
    model = select_arima_model(y, features)
    print(model.summary())

    fcst = forecast_area(model, y, features, FORECAST_HORIZON_DAYS)
    fcst = trim_forecast(fcst, alpha_threshold=99.95, km_ratio=0.01)

    day_change = compute_daily_change(occupied_by_ru)
    week_change, month_change = summarize_recent_changes(day_change)
    print(f"За последнюю неделю: {week_change:.2f}\nза последний месяц: {month_change:.2f}")

    plot_layout(occupied_by_ru, fcst, last_date, day_change)
    return 0


if __name__ == "__main__":
    sys.exit(main())
