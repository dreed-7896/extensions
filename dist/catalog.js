const DEFAULT_TITLE = "Suwatte Source List";
const DEFAULT_SORT = "name_asc";

const state = {
  allSources: [],
  query: "",
  sort: DEFAULT_SORT,
  language: "all",
};

const elements = {
  title: document.getElementById("catalog-title"),
  subtitle: document.getElementById("catalog-subtitle"),
  addAllButton: document.getElementById("add-all-button"),
  searchInput: document.getElementById("search-input"),
  sortSelect: document.getElementById("sort-select"),
  languageSelect: document.getElementById("language-select"),
  tableBody: document.getElementById("source-table-body"),
  emptyState: document.getElementById("empty-state"),
};

const normalizeCatalogUrl = () => {
  const withoutHash = window.location.href.split("#")[0];
  const withoutQuery = withoutHash.split("?")[0];
  const withoutIndex = withoutQuery.replace(/\/index\.html$/i, "");
  return withoutIndex.replace(/\/$/, "");
};

const redirectToCatalog = () => {
  const listUrl = encodeURIComponent(normalizeCatalogUrl());
  window.location.href = `suwatte://AddSourceList?list=${listUrl}`;
};

const redirectToSource = (sourcePath) => {
  const listUrl = encodeURIComponent(normalizeCatalogUrl());
  const encodedSource = encodeURIComponent(sourcePath);
  window.location.href =
    `suwatte://AddSourceFromList?list=${listUrl}&source=${encodedSource}`;
};

const sourceLanguages = (source) => {
  if (!Array.isArray(source.languages)) {
    return [];
  }
  return source.languages.filter((value) => typeof value === "string");
};

const sourceLanguageSubtitle = (source) => {
  const languages = sourceLanguages(source);
  if (!languages.length) {
    return "n/a";
  }
  return languages.join(", ");
};

const sourceThumbnail = (source) => {
  const thumbnail = source.thumbnail || source.thumbnailAssetPath;
  if (!thumbnail || typeof thumbnail !== "string") {
    return null;
  }
  if (/^https?:\/\//i.test(thumbnail)) {
    return thumbnail;
  }
  return `assets/${thumbnail.replace(/^\/+/, "")}`;
};

const asSearchable = (source) => {
  const languages = sourceLanguages(source).join(" ");
  return `${source.name || ""} ${source.id || ""} ${source.website || ""} ${languages}`.toLowerCase();
};

const compareSources = (left, right, sortMode) => {
  if (sortMode === "name_desc") {
    return String(right.name || "").localeCompare(String(left.name || ""));
  }
  if (sortMode === "version_desc") {
    return Number(right.version || 0) - Number(left.version || 0);
  }
  if (sortMode === "version_asc") {
    return Number(left.version || 0) - Number(right.version || 0);
  }
  return String(left.name || "").localeCompare(String(right.name || ""));
};

const filteredSources = () => {
  const query = state.query.trim().toLowerCase();

  const filtered = state.allSources.filter((source) => {
    if (state.language !== "all") {
      const languages = sourceLanguages(source);
      if (!languages.includes(state.language)) {
        return false;
      }
    }

    if (!query) {
      return true;
    }

    return asSearchable(source).includes(query);
  });

  filtered.sort((left, right) => compareSources(left, right, state.sort));
  return filtered;
};

const buildLanguageOptions = (sources) => {
  const unique = new Set();
  for (const source of sources) {
    for (const language of sourceLanguages(source)) {
      unique.add(language);
    }
  }

  const sortedLanguages = Array.from(unique).sort((a, b) => a.localeCompare(b));
  for (const language of sortedLanguages) {
    const option = document.createElement("option");
    option.value = language;
    option.textContent = language;
    elements.languageSelect.appendChild(option);
  }
};

const renderLanguages = (languages) => {
  if (!languages.length) {
    const span = document.createElement("span");
    span.className = "lang-pill";
    span.textContent = "n/a";
    return span;
  }

  const container = document.createElement("div");
  container.className = "lang-list";
  for (const value of languages) {
    const pill = document.createElement("span");
    pill.className = "lang-pill";
    pill.textContent = value;
    container.appendChild(pill);
  }

  return container;
};

const renderTableRows = (sources) => {
  elements.tableBody.innerHTML = "";

  const fragment = document.createDocumentFragment();
  for (const source of sources) {
    const languages = sourceLanguages(source);
    const row = document.createElement("tr");
    row.className = "source-row";

    const sourceCell = document.createElement("td");
    sourceCell.className = "cell-source";
    const sourceCellInner = document.createElement("div");
    sourceCellInner.className = "source-cell";

    const thumbnail = sourceThumbnail(source);
    if (thumbnail) {
      const image = document.createElement("img");
      image.className = "source-thumb";
      image.src = thumbnail;
      image.alt = `${source.name || "Source"} thumbnail`;
      image.loading = "lazy";
      sourceCellInner.appendChild(image);
    }

    const meta = document.createElement("div");
    meta.className = "source-meta";

    const nameRow = document.createElement("div");
    nameRow.className = "source-name-row";

    const name = document.createElement("h2");
    name.className = "source-name";
    name.textContent = String(source.name || "Unknown");
    nameRow.appendChild(name);

    const version = document.createElement("span");
    version.className = "source-version";
    version.textContent = `v${source.version || 0}`;
    nameRow.appendChild(version);

    meta.appendChild(nameRow);

    const id = document.createElement("span");
    id.className = "source-id";
    id.textContent = String(source.id || "");
    meta.appendChild(id);

    const subtitle = document.createElement("span");
    subtitle.className = "source-language-subtitle";
    subtitle.textContent = sourceLanguageSubtitle(source);
    meta.appendChild(subtitle);

    sourceCellInner.appendChild(meta);
    sourceCell.appendChild(sourceCellInner);
    row.appendChild(sourceCell);

    const languageCell = document.createElement("td");
    languageCell.className = "cell-languages";
    languageCell.appendChild(renderLanguages(languages));
    row.appendChild(languageCell);

    const websiteCell = document.createElement("td");
    websiteCell.className = "cell-website";
    if (source.website) {
      const link = document.createElement("a");
      link.href = String(source.website);
      link.target = "_blank";
      link.rel = "noopener noreferrer";
      link.textContent = String(source.website);
      websiteCell.appendChild(link);
    } else {
      websiteCell.textContent = "n/a";
    }
    row.appendChild(websiteCell);

    const actionCell = document.createElement("td");
    actionCell.className = "cell-action";
    const addButton = document.createElement("button");
    addButton.className = "button button--table";
    addButton.type = "button";
    addButton.textContent = "Add";
    addButton.addEventListener("click", () => {
      redirectToSource(String(source.path || ""));
    });
    actionCell.appendChild(addButton);
    row.appendChild(actionCell);

    fragment.appendChild(row);
  }

  elements.tableBody.appendChild(fragment);
};

const render = () => {
  const active = filteredSources();
  const total = state.allSources.length;
  const countLabel = active.length === 1 ? "source" : "sources";
  elements.subtitle.textContent = `${active.length} of ${total} ${countLabel}`;

  if (!active.length) {
    elements.emptyState.hidden = false;
  } else {
    elements.emptyState.hidden = true;
  }

  renderTableRows(active);
};

const validatePayload = (payload) => {
  if (!payload || typeof payload !== "object") {
    throw new Error("Invalid catalog payload");
  }
  if (!Array.isArray(payload.sources)) {
    throw new Error("Catalog payload is missing sources");
  }
  return payload;
};

const bindEvents = () => {
  elements.addAllButton.addEventListener("click", redirectToCatalog);

  elements.searchInput.addEventListener("input", (event) => {
    state.query = String(event.target.value || "");
    render();
  });

  elements.sortSelect.addEventListener("change", (event) => {
    state.sort = String(event.target.value || DEFAULT_SORT);
    render();
  });

  elements.languageSelect.addEventListener("change", (event) => {
    state.language = String(event.target.value || "all");
    render();
  });
};

const bootstrap = async () => {
  elements.sortSelect.value = DEFAULT_SORT;

  const response = await fetch("sources.json", { cache: "no-store" });
  if (!response.ok) {
    throw new Error(`Unable to load sources.json (${response.status})`);
  }

  const payload = validatePayload(await response.json());
  const listTitle = payload.listName || DEFAULT_TITLE;
  document.title = listTitle;
  elements.title.textContent = listTitle;

  state.allSources = payload.sources.slice();
  buildLanguageOptions(state.allSources);
  bindEvents();
  render();
};

bootstrap().catch((error) => {
  elements.subtitle.textContent = "Failed to load source list.";
  elements.emptyState.hidden = false;
  elements.emptyState.textContent =
    error instanceof Error ? error.message : "Unexpected catalog error.";
});
