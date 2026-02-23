import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
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

function App() {
  const [data, setData] = useState<TftData | null>(null);
  const [ownedItems, setOwnedItems] = useState<Record<string, number>>({});
  const [recommendations, setRecommendations] = useState<Recommendation[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

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
      setError(invokeError instanceof Error ? invokeError.message : "計算に失敗しました。");
    } finally {
      setLoading(false);
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
    </main>
  );
}

export default App;
