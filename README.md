# Kemable

Database client nhanh, nhẹ, mượt. Core Rust làm toàn bộ việc nặng, mỗi hệ điều hành có UI riêng trong `ui/`. Hiện chỉ có bản Windows (Tauri 2, Svelte 5, TypeScript).

## Chạy thử

Cần Rust 1.99+, Node 22+, pnpm và WebView2 (Windows 11 có sẵn).

```bash
cd ui/windows
pnpm install
pnpm tauri dev
```

Lần build đầu mất vài phút. Bấm **In memory** hoặc **Open SQLite…**, gõ SQL, rồi:

- `Ctrl+Enter`: chạy câu lệnh tại con trỏ (hoặc phần đang bôi đen)
- `Ctrl+Shift+Enter` / `F5`: chạy cả script
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
- `.claude/rules/`, `.claude/skills/`: rule theo đường dẫn và các skill `verify`, `add-driver`, `fix-issue`

## License

```
Kemable, a fast, light database client.
Copyright (C) 2026 Huỳnh Trung Đức

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU Affero General Public License as published
by the Free Software Foundation, version 3 of the License only.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU Affero General Public License for more details.

You should have received a copy of the GNU Affero General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
```

Toàn văn license ở `LICENSE` (AGPL-3.0-only). License thương mại, phần nào theo license nào: `LICENSING.md`. Tên và logo: `TRADEMARKS.md`. Thư viện bên thứ ba: `ui/windows/THIRD-PARTY-NOTICES.md`. Ai gửi pull request thì đồng ý với `CLA.md` (`CONTRIBUTING.md`). Kemable được phát triển dưới tổ chức [SoleBuilt](https://github.com/SoleBuilt).
