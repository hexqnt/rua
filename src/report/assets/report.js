(() => {
	const disclaimer = document.getElementById("data-disclaimer");
	if (disclaimer) {
		setTimeout(() => {
			disclaimer.classList.add("is-collapsed");
			const transitionDuration = window.matchMedia(
				"(prefers-reduced-motion: reduce)",
			).matches
				? 0
				: 350;
			setTimeout(() => {
				disclaimer.hidden = true;
			}, transitionDuration);
		}, 3000);
	}

	const applyGridDash = (plotId, layoutUpdate) => {
		const graph = document.getElementById(plotId);
		if (!graph || typeof Plotly === "undefined") return;

		const relayout = () => {
			Plotly.relayout(graph, layoutUpdate).catch(() => {});
		};

		if (graph.data && graph.layout) {
			relayout();
		} else {
			graph.on?.("plotly_afterplot", relayout);
			setTimeout(relayout, 50);
		}
	};

	applyGridDash("area-plot", {
		"xaxis.griddash": "dash",
		"xaxis2.griddash": "dash",
		"yaxis.griddash": "dash",
		"yaxis2.griddash": "dash",
	});
	applyGridDash("yoy-plot", {
		"xaxis.griddash": "dash",
		"xaxis2.griddash": "dash",
		"yaxis.griddash": "dash",
		"yaxis2.griddash": "dash",
	});

	const select = document.getElementById("ratio-sort");
	if (!select) return;

	const tables = Array.from(document.querySelectorAll(".ratio-table"));
	const sortTable = (table, key) => {
		const tbody = table.querySelector("tbody");
		if (!tbody) return;

		const rows = Array.from(tbody.querySelectorAll("tr.ratio-row"));
		rows.sort((a, b) => {
			if (key === "name") {
				return a.dataset.name.localeCompare(b.dataset.name, "ru");
			}
			return parseFloat(b.dataset.ratio) - parseFloat(a.dataset.ratio);
		});
		rows.forEach((row) => {
			tbody.appendChild(row);
		});
	};

	const applySort = () => {
		tables.forEach((table) => {
			sortTable(table, select.value);
		});
	};
	applySort();
	select.addEventListener("change", applySort);
})();
