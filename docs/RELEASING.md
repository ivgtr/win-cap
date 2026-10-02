# リリース手順

## 公開する

1. `Cargo.toml`と`Cargo.lock`の自分のパッケージのバージョンを更新します。未公開の初回v0.1.1では更新不要です。
2. [実機確認](DEVELOPMENT.md#検証記録と未確認事項)を行い、既知の制約をREADMEやCHANGELOGに反映します。
3. コミットを`main`へpushし、`Windows CI`の成功を確認します。
4. [ActionsのRelease](https://github.com/ivgtr/win-cap/actions/workflows/release.yml)で「Run workflow」を開き、公開するブランチ（通常は`main`）を選んで実行します。検査・ビルドの成功後に自動公開します。
5. `publish`ジョブと[Releases](https://github.com/ivgtr/win-cap/releases)のZIP・SHA-256・リリースノートを確認します。

`Release`ワークフローはデフォルトブランチに配置し、GitHub Actionsと公開ジョブの`contents: write`が許可されている必要があります。

## 公開の規則

- バージョンは`Cargo.toml`を正本とし、安定版の`MAJOR.MINOR.PATCH`形式だけを受け付けます。タグは`v<バージョン>`として自動作成します。
- `Release`は通常のCIと同じビルドを再利用し、実行開始時のコミットSHAに固定して公開します。通常のCIやタグpushでは公開しません。
- 書き込み権限は公開ジョブだけに付与し、認証には`GITHUB_TOKEN`を使います。
- リリース本文は[GitHub標準の自動生成](https://docs.github.com/en/repositories/releasing-projects-on-github/automatically-generated-release-notes)を使い、直前に公開された安定版のタグを比較元に指定します。初回は比較元を指定しません。PR一覧・貢献者・比較リンクが生成され、直接pushしたコミットは比較リンクで確認できます。
- CHANGELOGの追記は任意です。PRのタイトルはリリースノートにも載るため、変更内容が分かる名前にします。
- 同名のタグや公開済みリリースは上書きしません。新しいコミットがない場合も停止します（初回は対象外）。検査・チェックサム・GitHub APIの取得に失敗した場合も公開しません。

作成・添付の途中で失敗した場合は、残った下書きやタグが未公開であることと対象コミットを確認し、該当する下書き・タグを削除してから再実行します。公開済みの版を修正する場合は、新しいバージョンで公開してください。

## パッケージ

ZIPには実行ファイル、READMEと操作デモ、CHANGELOG、開発・公開の文書、MITライセンス、依存ライブラリーのライセンス表記を含めます。ZIPと`.zip.sha256`を公開し、Actionsの成果物は14日間保存します。

Windowsでビルドした後、Python 3.11以上でローカルのパッケージ処理を実行できます。

```sh
python .github/scripts/release.py check
python .github/scripts/release.py package
```

同名のZIP・SHA-256が`dist/`にある場合は、該当ファイルを削除してから再実行します。

ダウンロードしたZIPは、同じリリースのSHA-256とPowerShellで照合できます（v0.1.1の例）。

```powershell
$expected = (Get-Content win-cap-0.1.1-windows-x64.zip.sha256).Split(' ')[0]
$actual = (Get-FileHash win-cap-0.1.1-windows-x64.zip -Algorithm SHA256).Hash.ToLowerInvariant()
if ($actual -ne $expected) { throw 'ZIPのSHA-256が一致しません。再ダウンロードしてください。' }
```

CIはRustの`stable`を使うため、同じコミットを別の時点でビルドした際のバイナリー完全一致は保証しません。公式ActionsはコミットSHAで固定しています。
