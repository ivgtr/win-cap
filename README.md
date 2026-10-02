# win-cap

Windows向けの範囲録画アプリ。画面の一部を選び、音声なしのMP4として保存できます。

## ダウンロード・動作環境

[Releases](https://github.com/ivgtr/win-cap/releases)から`win-cap-<バージョン>-windows-x64.zip`をダウンロードし、展開して`win-cap.exe`を起動してください。インストールやRustの導入は不要です。

Windows 10 **2004以降** / Windows 11、x64に対応します。Windowsの画面取得とH.264エンコードが利用できる環境が必要です。

## 使い方

1. 「範囲を選択」を押し、画面をドラッグします。Escでキャンセルできます。
2. 30/60fpsとカーソルの有無を選び、「録画開始」を押します。
3. 操作画面が最小化されたら録画します。`Ctrl + Shift + F10`で停止します。
4. 停止後にMP4の保存先を選びます。

保存先の選択をキャンセルした場合や保存に失敗した場合は、「録画を保存」から再試行できます。保存を完了するまで次の録画は開始しません。既存のファイルは上書きしません。エラー時には原因と一時録画の場所を表示します。

`Ctrl + Shift + F9`でも開始・停止を切り替えられます。最小化された操作画面を戻し、「録画停止」を押すこともできます。

## 対応範囲・制約

- MP4 / H.264、音声なし。30/60fpsを上限に、画面の更新に応じて録画します。
- 1つのモニター内の48 × 48px以上の範囲。モニターをまたぐ選択は開始したモニターの端で切り、奇数の幅・高さは右端・下端を1px短くします。
- 幅または高さが256px未満ではCPU圧縮を使います。
- プレビュー、一時停止、編集、GIF出力は未対応です。

長時間録画・複数モニター・負荷比較は未検証です。

## ビルド

[Rust](https://www.rust-lang.org/tools/install)のMSVCツールチェーンと、Visual Studio Build Toolsの「C++によるデスクトップ開発」、Windows SDKが必要です。

```powershell
cargo build --release --locked --target x86_64-pc-windows-msvc
```

成果物は`target/x86_64-pc-windows-msvc/release/win-cap.exe`です。

検証状況と開発用の検査は[開発・検証](docs/DEVELOPMENT.md)、Actionsからの公開方法は[リリース手順](docs/RELEASING.md)を参照してください。

変更履歴は[CHANGELOG](CHANGELOG.md)、ライセンスは[MIT](LICENSE)です。依存ライブラリーのライセンス表記は[THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt)に同梱しています。
