# Kemable

Database client nhanh, nhẹ, mượt. Core Rust làm toàn bộ việc nặng, mỗi hệ điều hành có UI riêng trong `ui/`. Hiện chỉ có bản Windows (Tauri 2, Svelte 5, TypeScript).

[English](README.md) · [Website](https://kemable.pages.dev) · [Tải về](https://github.com/SoleBuilt/Kemable/releases/latest)

## Tải về

Kemable 0.1 là bản preview cho **Windows 10 và 11, x64**. Tải `Kemable_<version>_x64-setup.exe` ở [bản phát hành mới nhất](https://github.com/SoleBuilt/Kemable/releases/latest) rồi chạy. Installer hỏi cài cho riêng bạn hay cho mọi người, và cài ở đâu.

Installer chưa được ký số (code signing), nên Windows SmartScreen có thể báo "Windows protected your PC": bấm **More info**, rồi **Run anyway**. Sau khi cài, Kemable tự cập nhật: mỗi lần mở, app hỏi GitHub một lần xem có bản mới không.

Engine: SQLite, PostgreSQL (cả Neon và Supabase), SQL Server, Redis, Cloudflare D1; RabbitMQ và Firebase ở dạng preview.

## Chạy từ mã nguồn

Cần Rust 1.99 (rustup tự cài theo `rust-toolchain.toml`), Node 22+, pnpm và WebView2 (Windows 11 có sẵn).

```bash
cd ui/windows
pnpm install
pnpm tauri dev
```

Lần build đầu mất vài phút. Ở màn hình Welcome, bấm **Open an in-memory database** (hoặc **Open file…** để mở một file SQLite, **New connection…** để kết nối tới server), gõ SQL, rồi:

- `Ctrl+Enter`: chạy câu lệnh tại con trỏ (hoặc phần đang bôi đen)
- `Ctrl+Shift+Enter` / `F5`: chạy cả script
- `Ctrl+K`: tìm kết nối và lệnh
- Trên grid: phím mũi tên, `Ctrl+C` để copy ô, kéo mép header để đổi độ rộng cột

Thử tốc độ với 2 triệu dòng:

```sql
with recursive c(x) as (select 1 union all select x + 1 from c limit 2000000)
select x, x * 2 as doubled, 'row ' || x as label from c
```

## Kiểm tra trước khi xong một thay đổi

```bash
cargo install --locked cargo-deny; cargo install --locked --features cli cargo-about   # một lần: kiểm tra license của thư viện
node scripts/verify.mjs
```

## Tài liệu

- `AGENTS.md`: quy tắc chung (Claude Code đọc qua `CLAUDE.md`)
- `core/ARCHITECTURE.md`: kiến trúc core
- `docs/decisions/`: các quyết định kiến trúc (ADR)
- `docs/roadmap.md`: lộ trình và danh sách engine
- `docs/testing/`: danh sách kiểm thử thủ công theo từng phase (`phase-1.md`)
- `site/`: website của Kemable (`site/README.md`)
- `.claude/rules/`, `.claude/skills/`: rule theo đường dẫn và các skill `verify`, `add-driver`, `fix-issue`, `clean-target`

## License

Kemable là phần mềm tự do theo [GNU Affero General Public License, chỉ phiên bản 3](LICENSE) (AGPL-3.0-only), đồng thời có license thương mại. Phần nào theo license nào, và cách hỏi license thương mại: `LICENSING.md`. Tên và logo: `TRADEMARKS.md`. Thư viện bên thứ ba: `ui/windows/THIRD-PARTY-NOTICES.md`. Ai gửi pull request thì đồng ý với `CLA.md` (`CONTRIBUTING.md`). Kemable là một dự án của [SoleBuilt](https://solebuilt.ducht.info.vn/). Bản tiếng Anh ([README.md](README.md)) có toàn văn thông báo license.
