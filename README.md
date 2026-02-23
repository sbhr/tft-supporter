# TFT Supporter (Tauri + React)

TFTの所持アイテムから、おすすめ構成（上位5件）を提示するWindows向けデスクトップアプリです。

## MVPの仕様

- 入力: コンポーネントアイテムを手動で増減
- 推薦基準: 完成可能性優先（作成可能なコアアイテム数が多い順）
- 出力: 上位5構成、作成可能コア、不足素材
- データ源: ローカル固定JSON（手動更新）
- 外部取得: MetaTFT構成一覧を取得し、構成名/チャンピオン/アイテムを推定分離
- 推定: 必須/優先アイテム、エース想定、AD/APスタイル判定
- 提案: 所持素材から「行きやすさスコア」で上位構成を提示
- 保存: 分析結果をローカルJSONへ書き出し、次回以降に再利用

## 起動方法

```bash
npm install
npm run tauri dev
```

Webのみ確認する場合:

```bash
npm run dev
```

## データ更新

データファイルは [public/data/tft-data.json](public/data/tft-data.json) です。

- `components`: 基本コンポーネント
- `completedItems`: 完成アイテムとレシピ（2素材）
- `comps`: 構成候補とコアアイテム

このJSONを差し替えることで、推薦対象を更新できます。

## 実装ポイント

- フロントUI: [src/App.tsx](src/App.tsx)
- スタイル: [src/App.css](src/App.css)
- 推薦ロジック(Tauri command): [src-tauri/src/lib.rs](src-tauri/src/lib.rs)

## 外部ティアリスト分析

アプリ内の「MetaTFTを分析」ボタンで、以下URLから構成ティア一覧を取得・解析します。

- `https://meta-tft.com/en/decks/?period=7&tiers=EMERALD&tiers=DIAMOND&tiers=MASTER&tiers=GRANDMASTER&tiers=CHALLENGER`

取得処理は Tauri 側でHTMLのテーブル行を解析し、以下を推定します。

- 構成名（先頭2語）
- チャンピオン一覧（重複出現パターンから抽出）
- 必須/優先アイテム（先頭装備 + 頻出度の混合ルール）
- AD/AP（アイテム辞書ベース）
- 所持素材との適合度（作成可能数・不足素材）

## ローカル保存と再利用

- 「MetaTFTを分析して保存」を押すと、分析結果をアプリのローカル保存領域に `meta-deck-analysis.json` として保存します。
- 「保存済みを読込」で、最後に保存した分析結果を再表示できます。
- 保存ファイルの絶対パスはアプリ画面の「保存先」に表示されます。
- 「構成ファイルへ反映」で、保存済み分析から `generated-recommended-comps.json` を生成します。
- 生成後は `recommend_comps` がこのファイルを自動で読み込み、ローカル固定 `comps` と合わせて推薦に利用します。
