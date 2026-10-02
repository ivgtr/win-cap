# 開発・検証

動作環境とビルド方法は[README](../README.md)を参照してください。

## 構成

```text
Windows Graphics Capture
  → Direct3D 11で指定範囲をGPU上で切り出し
  → 最大2枚の待機キュー
  → MediaStreamSource / MediaTranscoder
  → MP4 / H.264
```

画面取得・切り出しではCPUへ画素を読み戻さず、GPUサーフェスをWindowsの動画処理へ渡します。エンコード中のテクスチャーは上書きせず、キューが満杯なら新しいフレームを省略します。静止画面でも終了時刻のサンプルを渡し、停止までの時間を動画に残します。

幅または高さが256px未満では、小範囲に対応できないハードウェアエンコーダーを避けるためCPU圧縮を使います。256 × 256px以上ではハードウェア圧縮を有効にしますが、実際の選択はWindows・GPU・ドライバーに依存します。画面取得自体は、範囲の大きさによらずモニター全体です。

録画はWindowsの一時フォルダーに書き込み、停止後に選んだ保存先へコピーします。コピー完了を確認してから一時録画を削除し、保存のキャンセルやコピー失敗では保持します。録画中にアプリを閉じた場合も停止後に保存先を選び、保存せず終了した場合は一時録画の場所を表示します。録画エラーで途中終了したMP4は再生できない場合があります。

参照: [Microsoftの録画構成](https://learn.microsoft.com/en-us/windows/uwp/audio-video-camera/screen-capture-video)、[Windows Capture](https://github.com/NiiightmareXD/windows-capture)。

## 自動検査

Windowsで次を実行します。`Windows CI`も同じRustの検査とリリース用ビルドを実行します。

```powershell
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
```

パッケージ処理の検査にはPython 3.11以上を使います。

```sh
python .github/scripts/test_release.py
```

GPU上の合成画像を使ったMP4生成テストは、Windows実機で明示的に実行します。画面の内容は取得せず、通常のCIでは実行しません。

```powershell
cargo test --locked --test encoding -- --ignored --nocapture
```

## 操作デモの再生成

READMEの[GIF](assets/demo.gif)と[MP4](assets/demo.mp4)は、`src/ui.rs`の配置・文言・操作順を基準にした操作モックです。Windows実機での録画・保存の検証には使いません。モックと制作ツールは[`tools/demo`](../tools/demo/)に置き、製品の依存関係とは分離しています。

Node.js 22以上と日本語フォントを用意し、リポジトリのルートから実行します。依存バージョンは`tools/demo/package-lock.json`で固定しています。

```sh
mkdir -p tools/demo/.cache tools/demo/.work
XDG_CACHE_HOME="$PWD/tools/demo/.cache" TMPDIR="$PWD/tools/demo/.work" \
  npm --prefix tools/demo ci --cache "$PWD/tools/demo/.cache"
npm --prefix tools/demo exec -- playwright install chromium
npm --prefix tools/demo run preview
npm --prefix tools/demo run render
```

`preview`は実操作の検査と開始・中間・結果・ループの静止画を`tools/demo/.work/previews/`へ出力します。`render`は同じ検査後に両形式を生成し、フレーム数・尺・最後までのデコードを確認します。生成後の映像から取り出した静止画も同じフォルダーに残るので、掲載サイズで日本語とカーソル位置を確認してください。

既存のChromiumを指定する場合は`npm --prefix tools/demo run render -- --browser-executable /path/to/chromium`を使います。LinuxではChromiumが必要とするシステムライブラリーと日本語フォントも必要です。今回の生成にはDroid Sans Fallbackを使っています。

制作条件は20秒・音声なし。MP4は1024 × 704px・24fps・H.264、GIFは960 × 660px・12fps相当・無限ループです。[`mock.js`](../tools/demo/mock.js)の状態遷移と時刻指定描画を共有し、「範囲選択 → 録画 → ホットキーで停止 → MP4保存」を撮影します。アプリの表示バージョンは`Cargo.toml`から取得します。画面や操作を変更したら両形式を再生成してください。

## 検証記録と未確認事項

v0.1.1では、小範囲の圧縮経路の修正後に48 × 48、64 × 64、104 × 84、256 × 256、640 × 480pxの合成画像から、サイズを変えずMP4を生成できることと、1秒の静止映像の尺を確認しました。

操作画面では104 × 84pxの範囲で、開始時に保存ダイアログが出ないこと、停止後の保存先選択、キャンセル後の再保存と完了通知を確認しました。画質と実再生、長時間録画、複数モニター、負荷比較は未確認です。

実機確認では次の条件を使います。

| 確認 | 期待する結果 |
| --- | --- |
| 録画開始 | 保存先を指定せず録画が始まる |
| 通常の範囲を10秒録画して停止 | 選択範囲のMP4が再生でき、尺がほぼ10秒 |
| 保存をキャンセルして再試行 | 「録画を保存」から保存し直せる |
| 静止画面を10秒録画 | 停止までの時間が動画に残る |
| 100%/150%表示、左側・上側のモニター | 選択範囲と実際の映像が一致する |
| カーソルを含む/含まない | 設定に従って映像に入る |
| 60fps、大きい範囲、5分以上 | メモリーが時間に比例して増え続けない |
| 開始直後の停止、録画中にアプリを閉じる | UIが固まらず、保存完了または理由付きエラー |
| モニター切断、解像度変更、書き込み失敗 | 理由と一時ファイルの場所を表示する |
| 同じ保存名への再録画 | 既存の動画を上書きしない |

負荷比較は同じPC・範囲・fps・映像内容・録画時間で、CPU、GPUのVideo Encode、メモリー、出力サイズと再生の滑らかさを比べます。他の録画アプリとの軽さの比較は未測定です。
