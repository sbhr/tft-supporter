# TFT Supporter (Tauri + React)

TFTの所持アイテムから、おすすめ構成（上位5件）を提示するWindows向けデスクトップアプリです。

## MVPの仕様

- 入力: コンポーネントアイテムを手動で増減
- 推薦基準: 完成可能性優先（作成可能なコアアイテム数が多い順）
- 出力: 上位5構成、作成可能コア、不足素材
- データ源: ローカル固定JSON（手動更新）

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
