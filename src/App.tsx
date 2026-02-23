import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import "./App.css";

type ComponentItem = {
  id: string;
  name: string;
};

type TftData = {
  components: ComponentItem[];
  completedItems: { id: string; name: string }[];
};

type MissingComponent = {
  item_id: string;
  missing_count: number;
};

type Recommendation = {
  comp_id: string;
  comp_name: string;
  description: string;
  crafted_count: number;
  total_core_items: number;
  missing_components: MissingComponent[];
  craftable_items: string[];
  score: number;
};

type RecommendResponse = {
  recommendations: Recommendation[];
};

type MetaTftDeckTierEntry = {
  comp_name: string;
  tier: string;
  avg_placement: string;
  top4_rate: string;
  games: string;
  ace_champion: string;
  champions: string[];
  mandatory_items: string[];
  priority_items: string[];
  style: string;
  craftable_target_items: number;
  missing_components: MissingComponent[];
  fit_score: number;
};

type MetaTftDeckRecommendation = {
  comp_name: string;
  tier: string;
  style: string;
  ace_champion: string;
  mandatory_items: string[];
  priority_items: string[];
  craftable_target_items: number;
  missing_components: MissingComponent[];
  fit_score: number;
};

type MetaTftDeckTierResponse = {
  source_url: string;
  entries: MetaTftDeckTierEntry[];
  recommendations: MetaTftDeckRecommendation[];
  cache_file_path: string;
  loaded_from_cache: boolean;
};

type ExportLocalCompsResponse = {
  file_path: string;
  exported_count: number;
};

type DefaultGeneratedPathResponse = string;

const FIXED_COMPONENT_ORDER = [
  "bf_sword",
  "recurve_bow",
  "needlessly_large_rod",
  "tear_of_the_goddess",
  "chain_vest",
  "negatron_cloak",
  "giants_belt",
  "sparring_gloves",
  "spatula",
  "frying_pan",
] as const;

function extractInvokeErrorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error && error.message.trim().length > 0) {
    return error.message;
  }

  if (typeof error === "string" && error.trim().length > 0) {
    return error;
  }

  if (error && typeof error === "object") {
    const record = error as Record<string, unknown>;
    const candidates = [record.message, record.error, record.cause, record.details];
    for (const candidate of candidates) {
      if (typeof candidate === "string" && candidate.trim().length > 0) {
        return candidate;
      }
    }

    try {
      const serialized = JSON.stringify(error);
      if (serialized && serialized !== "{}") {
        return `${fallback} (${serialized})`;
      }
    } catch {
    }
  }

  return fallback;
}

function App() {
  const [data, setData] = useState<TftData | null>(null);
  const [ownedItems, setOwnedItems] = useState<Record<string, number>>({});
  const [recommendations, setRecommendations] = useState<Recommendation[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [metaRows, setMetaRows] = useState<MetaTftDeckTierEntry[]>([]);
  const [metaRecommendations, setMetaRecommendations] = useState<MetaTftDeckRecommendation[]>([]);
  const [metaSourceUrl, setMetaSourceUrl] = useState<string>("");
  const [metaCachePath, setMetaCachePath] = useState<string>("");
  const [metaLoadedFromCache, setMetaLoadedFromCache] = useState(false);
  const [metaLoading, setMetaLoading] = useState(false);
  const [metaError, setMetaError] = useState<string | null>(null);
  const [exportMessage, setExportMessage] = useState<string>("");
  const [exportTargetPath, setExportTargetPath] = useState<string>("");

  useEffect(() => {
    void loadSavedMetaAnalysis();
    void loadDefaultGeneratedPath();
  }, []);

  async function loadDefaultGeneratedPath() {
    try {
      const path = await invoke<DefaultGeneratedPathResponse>("default_generated_comps_path");
      setExportTargetPath(path);
    } catch {
    }
  }

  async function loadSavedMetaAnalysis(showError = false) {
    try {
      const response = await invoke<MetaTftDeckTierResponse>("load_saved_meta_tft_decks");
      setMetaRows(response.entries);
      setMetaRecommendations(response.recommendations);
      setMetaSourceUrl(response.source_url);
      setMetaCachePath(response.cache_file_path);
      setMetaLoadedFromCache(response.loaded_from_cache);
    } catch (invokeError) {
      if (showError) {
        setMetaError(extractInvokeErrorMessage(invokeError, "保存済みデータの読込に失敗しました。"));
      }
    }
  }

  useEffect(() => {
    fetch("/data/tft-data.json")
      .then(async (response) => {
        if (!response.ok) {
          throw new Error("データファイルの読み込みに失敗しました。");
        }
        return (await response.json()) as TftData;
      })
      .then((json) => {
        setData(json);
      })
      .catch((fetchError) => {
        setError(fetchError instanceof Error ? fetchError.message : "不明なエラーです。");
      });
  }, []);

  const componentNameMap = useMemo(() => {
    const map = new Map<string, string>();
    data?.components.forEach((component) => {
      map.set(component.id, component.name);
    });
    return map;
  }, [data]);

  const completedItemNameMap = useMemo(() => {
    const map = new Map<string, string>();
    data?.completedItems.forEach((item) => {
      map.set(item.id, item.name);
    });
    return map;
  }, [data]);

  const sortedComponents = useMemo(() => {
    if (!data) {
      return [];
    }

    const orderMap = new Map<string, number>();
    FIXED_COMPONENT_ORDER.forEach((itemId, index) => {
      orderMap.set(itemId, index);
    });

    return [...data.components].sort((left, right) => {
      const leftOrder = orderMap.get(left.id) ?? Number.MAX_SAFE_INTEGER;
      const rightOrder = orderMap.get(right.id) ?? Number.MAX_SAFE_INTEGER;
      return leftOrder - rightOrder;
    });
  }, [data]);

  const totalOwnedCount = useMemo(
    () => Object.values(ownedItems).reduce((sum, count) => sum + count, 0),
    [ownedItems],
  );

  function changeOwnedCount(itemId: string, delta: number) {
    setOwnedItems((current) => {
      const nextCount = Math.max(0, (current[itemId] ?? 0) + delta);
      const next = { ...current };
      if (nextCount === 0) {
        delete next[itemId];
      } else {
        next[itemId] = nextCount;
      }
      return next;
    });
  }

  async function calculateRecommendations() {
    setError(null);
    setLoading(true);
    try {
      const response = await invoke<RecommendResponse>("recommend_comps", {
        payload: { ownedItems },
      });
      setRecommendations(response.recommendations);
    } catch (invokeError) {
      setError(extractInvokeErrorMessage(invokeError, "計算に失敗しました。"));
    } finally {
      setLoading(false);
    }
  }

  async function fetchMetaTftTierList() {
    setMetaError(null);
    setMetaLoading(true);
    try {
      const response = await invoke<MetaTftDeckTierResponse>("analyze_meta_tft_decks", {
        payload: { ownedItems },
      });
      setMetaRows(response.entries);
      setMetaRecommendations(response.recommendations);
      setMetaSourceUrl(response.source_url);
      setMetaCachePath(response.cache_file_path);
      setMetaLoadedFromCache(response.loaded_from_cache);
    } catch (invokeError) {
      setMetaError(extractInvokeErrorMessage(invokeError, "MetaTFTの取得に失敗しました。"));
    } finally {
      setMetaLoading(false);
    }
  }

  async function exportSavedMetaToLocalComps() {
    setMetaError(null);
    setExportMessage("");
    try {
      const response = await invoke<ExportLocalCompsResponse>("export_saved_meta_to_local_recommendations", {
        targetPath: exportTargetPath.trim().length > 0 ? exportTargetPath.trim() : null,
      });
      setExportTargetPath(response.file_path);
      setExportMessage(`ローカル構成ファイルを出力しました: ${response.exported_count}件 (${response.file_path})`);
    } catch (invokeError) {
      setMetaError(extractInvokeErrorMessage(invokeError, "ローカル構成ファイルの出力に失敗しました。"));
    }
  }

  async function chooseExportPath() {
    setMetaError(null);
    try {
      const selected = await save({
        defaultPath: exportTargetPath || undefined,
        filters: [{ name: "JSON", extensions: ["json"] }],
      });

      if (typeof selected === "string" && selected.trim().length > 0) {
        setExportTargetPath(selected);
      }
    } catch (dialogError) {
      setMetaError(extractInvokeErrorMessage(dialogError, "保存先ダイアログの表示に失敗しました。"));
    }
  }

  return (
    <main className="app">
      <h1>TFT Build Supporter</h1>
      <p className="lead">所持アイテムから、進行先としておすすめの構成を提示します。</p>

      {error ? <p className="error">{error}</p> : null}

      <section className="panel">
        <div className="panel-header">
          <h2>所持アイテム</h2>
          <span>合計 {totalOwnedCount} 個</span>
        </div>

        <div className="item-grid">
          {sortedComponents.map((component) => {
            const count = ownedItems[component.id] ?? 0;
            return (
              <div key={component.id} className="item-card">
                <p>{component.name}</p>
                <div className="counter">
                  <button type="button" onClick={() => changeOwnedCount(component.id, -1)}>
                    -
                  </button>
                  <span>{count}</span>
                  <button type="button" onClick={() => changeOwnedCount(component.id, 1)}>
                    +
                  </button>
                </div>
              </div>
            );
          })}
        </div>

        <button
          className="primary"
          type="button"
          onClick={() => void calculateRecommendations()}
          disabled={loading || totalOwnedCount === 0}
        >
          {loading ? "計算中..." : "おすすめを計算"}
        </button>
      </section>

      <section className="panel">
        <h2>おすすめ構成（上位5件）</h2>
        {recommendations.length === 0 ? (
          <p className="muted">アイテムを入力して「おすすめを計算」を押してください。</p>
        ) : (
          <div className="result-list">
            {recommendations.map((recommendation, index) => (
              <article key={recommendation.comp_id} className="result-card">
                <div className="result-title">
                  <h3>
                    #{index + 1} {recommendation.comp_name}
                  </h3>
                  <span>
                    完成可能 {recommendation.crafted_count}/{recommendation.total_core_items}
                  </span>
                </div>
                <p>{recommendation.description}</p>
                <p className="muted">Score: {recommendation.score}</p>
                <p>
                  作成可能コア: {recommendation.craftable_items.length > 0
                    ? recommendation.craftable_items
                        .map((itemId) => completedItemNameMap.get(itemId) ?? itemId)
                        .join(", ")
                    : "なし"}
                </p>
                <p>
                  不足素材:
                  {recommendation.missing_components.length === 0
                    ? " なし"
                    : ` ${recommendation.missing_components
                        .map(
                          (component) =>
                            `${componentNameMap.get(component.item_id) ?? component.item_id} x${component.missing_count}`,
                        )
                        .join(", ")}`}
                </p>
              </article>
            ))}
          </div>
        )}
      </section>

      <section className="panel">
        <div className="panel-header">
          <h2>MetaTFT 構成分析</h2>
          <div className="button-row">
            <button className="primary" type="button" onClick={() => void fetchMetaTftTierList()} disabled={metaLoading}>
              {metaLoading ? "分析中..." : "MetaTFTを分析して保存"}
            </button>
            <button type="button" onClick={() => void loadSavedMetaAnalysis(true)} disabled={metaLoading}>
              保存済みを読込
            </button>
            <button type="button" onClick={() => void exportSavedMetaToLocalComps()} disabled={metaLoading}>
              構成ファイルへ反映
            </button>
          </div>
        </div>

        {metaError ? <p className="error">{metaError}</p> : null}
        {metaSourceUrl ? <p className="muted">取得元: {metaSourceUrl}</p> : null}
        {metaCachePath ? <p className="muted">保存先: {metaCachePath}</p> : null}
        {metaLoadedFromCache ? <p className="muted">表示中: 保存済みデータ</p> : null}
        {exportMessage ? <p className="muted">{exportMessage}</p> : null}
        <div className="path-row">
          <label htmlFor="export-path">構成ファイルの保存先</label>
          <div className="path-controls">
            <input
              id="export-path"
              value={exportTargetPath}
              onChange={(event) => setExportTargetPath(event.target.value)}
              placeholder="保存先パス"
            />
            <button type="button" onClick={() => void chooseExportPath()} disabled={metaLoading}>
              参照
            </button>
          </div>
        </div>

        {metaRows.length === 0 ? (
          <p className="muted">ボタンを押すと、指定URLの構成データを解析して提案を表示します。</p>
        ) : (
          <div className="meta-analysis">
            <div className="result-list">
              {metaRecommendations.map((recommendation, index) => (
                <article key={`${recommendation.comp_name}-${index}`} className="result-card">
                  <div className="result-title">
                    <h3>
                      #{index + 1} {recommendation.comp_name}
                    </h3>
                    <span>
                      Tier {recommendation.tier} / {recommendation.style}
                    </span>
                  </div>
                  <p>エース想定: {recommendation.ace_champion}</p>
                  <p className="muted">行きやすさスコア: {recommendation.fit_score}</p>
                  <p>必須推定: {recommendation.mandatory_items.join(", ") || "なし"}</p>
                  <p>優先推定: {recommendation.priority_items.join(", ") || "なし"}</p>
                  <p>作成可能目標アイテム: {recommendation.craftable_target_items}</p>
                  <p>
                    不足素材:
                    {recommendation.missing_components.length === 0
                      ? " なし"
                      : ` ${recommendation.missing_components
                          .map(
                            (component) =>
                              `${componentNameMap.get(component.item_id) ?? component.item_id} x${component.missing_count}`,
                          )
                          .join(", ")}`}
                  </p>
                </article>
              ))}
            </div>

            <div className="meta-table-wrap">
              <table className="meta-table">
                <thead>
                  <tr>
                    <th>構成名</th>
                    <th>Tier</th>
                    <th>AD/AP</th>
                    <th>エース想定</th>
                    <th>チャンピオン</th>
                    <th>必須推定</th>
                    <th>優先推定</th>
                    <th>平均順位</th>
                    <th>Top4率</th>
                    <th>試合数</th>
                    <th>行きやすさ</th>
                  </tr>
                </thead>
                <tbody>
                  {metaRows.map((row, index) => (
                    <tr key={`${row.tier}-${row.comp_name}-${index}`}>
                      <td>{row.comp_name}</td>
                      <td>{row.tier}</td>
                      <td>{row.style}</td>
                      <td>{row.ace_champion}</td>
                      <td>{row.champions.join(", ") || "-"}</td>
                      <td>{row.mandatory_items.join(", ") || "-"}</td>
                      <td>{row.priority_items.join(", ") || "-"}</td>
                      <td>{row.avg_placement}</td>
                      <td>{row.top4_rate}</td>
                      <td>{row.games}</td>
                      <td>{row.fit_score}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>
        )}
      </section>
    </main>
  );
}

export default App;
