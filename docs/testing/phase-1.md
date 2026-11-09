# Kiểm thử thủ công Phase 1

Danh sách những gì cần thử bằng tay trên app, cách thử và kết quả đúng. Các phần chạy được tự động (core, driver, store) đã có test và chạy bằng `node scripts/verify.mjs` cùng các test live ở cuối file; tài liệu này là phần chỉ có người bấm mới kiểm được: giao diện, chuột, bàn phím, clipboard.

Viết bằng tiếng Việt vì đây là tài liệu cho người kiểm thử đọc, không phải quy tắc cho agent.

**Cách ghi kết quả:** đánh dấu `[x]` khi đúng. Khi sai, ghi ngay dưới mục đó: đã làm gì, thấy gì, mong đợi gì, kèm ảnh chụp nếu là lỗi giao diện. Sau đó nhờ agent dùng skill `fix-issue`.

---

## 0. Chuẩn bị

1. Mở terminal ở `ui/windows` và chạy `pnpm tauri dev`. Lần đầu build mất vài phút.
2. Có sẵn trên máy (đã dùng khi làm Phase 1):
   - PostgreSQL 17 ở `localhost:5432`, user `postgres`, database `demo_sales` có dữ liệu mẫu (`scripts/demo/postgres-demo-sales.sql`).
   - SQL Server 2025 ở `localhost`, đăng nhập bằng Windows Authentication.
3. Mở DevTools khi cần xem lỗi JavaScript: chuột phải trong app, chọn **Inspect** (chỉ có ở bản dev).

---

## 1. Kết nối

### 1.1 SQLite
- [ ] Bấm **In memory**, gõ `select 1 as one, 'xin chào' as greeting;`, nhấn **Ctrl+Enter**.
  **Đúng:** grid hiện 1 dòng, chữ tiếng Việt hiển thị đúng; toolbar ghi "In memory · SQLite 3.x".
- [ ] Bấm **Open SQLite…**, chọn một file `.db` hoặc `.sqlite` có sẵn.
  **Đúng:** sidebar hiện schema `main` và các bảng; double-click một bảng thì grid hiện dữ liệu.

### 1.2 PostgreSQL
- [ ] Bấm **PostgreSQL…**, dán URL `postgres://postgres:<mật khẩu>@localhost:5432/postgres` vào ô URL.
  **Đúng:** Host, Port, User, Password tự điền; ô Database để trống cũng được.
- [ ] Nhập Name `Local PG`, giữ tick **Save this connection** và **Keep the password…**, bấm **Connect**.
  **Đúng:** kết nối thành công; sidebar hiện **tất cả database** (`demo_sales`, `postgres`), database đang dùng in đậm, có chấm xanh.
- [ ] Mở `demo_sales` → `public`.
  **Đúng:** 6 bảng `categories, customers, order_items, orders, payments, products` cùng các sequence.
- [ ] Thử sai mật khẩu.
  **Đúng:** hộp thoại báo "Access denied" và dòng lỗi của Postgres, không treo.

### 1.3 SQL Server
- [ ] Bấm **SQL Server…**, Host `localhost`, Authentication **Windows Authentication**, Name `Local MSSQL`, bấm **Connect**.
  **Đúng:** sidebar hiện `master`, `model`, `msdb`, `tempdb` (và database của bạn nếu có); database người dùng đứng trước database hệ thống.
- [ ] Nếu có instance `SERVERAPP`: Host `localhost\SERVERAPP`, để trống Port.
  **Đúng:** kết nối qua SQL Server Browser.

### 1.4 Connection đã lưu
- [ ] Bấm **Disconnect** trên toolbar.
  **Đúng:** sidebar hiện **Saved connections**, `Local PG` và `Local MSSQL` ở đầu (mới dùng nhất đứng trước).
- [ ] Double-click `Local PG`.
  **Đúng:** kết nối ngay, không hỏi mật khẩu.
- [ ] Mở **Windows Credential Manager** → **Windows Credentials** → **Generic Credentials**.
  **Đúng:** có mục tên kiểu `connection/<id>/password.Paboable`.
- [ ] Mở `%APPDATA%\dev.paboable.app\connections.json` bằng Notepad.
  **Đúng:** có connection nhưng **không có mật khẩu**.
- [ ] Bấm ✎ trên một connection, đổi tên, bỏ tick **Keep the password…**, bấm **Save**.
  **Đúng:** mục trong Credential Manager biến mất; double-click connection đó thì app hỏi mật khẩu.
- [ ] Bấm ✕, rồi **Yes**.
  **Đúng:** connection biến mất khỏi danh sách.

---

## 2. Chạy query

- [ ] Gõ 3 câu trên 3 dòng, đặt con trỏ ở câu giữa, nhấn **Ctrl+Enter**.
  **Đúng:** chỉ câu giữa chạy, câu đó được tô sáng rồi mờ dần; status bar ghi "Ran 1 of 3 statements".
- [ ] Nhấn **Ctrl+Shift+Enter** (hoặc **F5**).
  **Đúng:** cả 3 câu chạy, có 3 tab Result.
- [ ] Bôi đen một phần rồi Ctrl+Enter.
  **Đúng:** chỉ phần bôi đen chạy.
- [ ] Postgres: `select pg_sleep(30);`, bấm **■ Stop** sau 1–2 giây.
  **Đúng:** dừng ngay; Messages báo "Stopped"; chạy `select 1` tiếp được.
- [ ] SQL Server: `waitfor delay '00:00:30'`, bấm **■ Stop**.
  **Đúng:** như trên.
- [ ] Gõ sai cú pháp, ví dụ `select * frm customers`.
  **Đúng:** tab Messages báo "The query failed" kèm lỗi của database.
- [ ] Postgres: `do $$ begin raise notice 'xin chào'; end $$;`
  **Đúng:** Messages có dòng `xin chào`.
- [ ] SQL Server: script có `GO`:
  ```sql
  select 1 as a
  GO
  select 2 as b
  ```
  **Đúng:** 2 tab Result.
- [ ] F5 và Ctrl+R **không** tải lại cả trang (app không mất kết nối).

---

## 3. Database và sidebar

- [ ] Bấm nút database trên toolbar (cạnh tên server), hoặc **Ctrl+K**, gõ để lọc, ↑↓ rồi Enter.
  **Đúng:** editor chuyển sang database đó; status bar ghi "in <database>"; nhánh của database đó trên sidebar mở ra.
- [ ] Double-click một database trên sidebar.
  **Đúng:** giống như trên.
- [ ] Double-click một bảng thuộc database khác database đang dùng.
  **Đúng:** editor chuyển sang database đó rồi mở bảng.
- [ ] Gõ vào ô **Filter** của sidebar.
  **Đúng:** chỉ còn các bảng có tên khớp.
- [ ] Chạy `create table thu_refresh (x int);` rồi xem sidebar.
  **Đúng:** bảng mới tự hiện. Nhấn **Ctrl+R** hoặc nút ↻ cũng làm mới được.

### 3.1 Script tạo database
- [ ] Postgres, Run all:
  ```sql
  CREATE DATABASE thu_tu_dong;
  create table bang_thu (id int);
  ```
  **Đúng:** Messages có "Switched to thu_tu_dong…"; `bang_thu` nằm trong `thu_tu_dong`, không nằm trong `postgres`.
- [ ] Từ database `postgres`, chạy `DROP DATABASE thu_tu_dong;`.
  **Đúng:** xóa được, không báo "being accessed by other users".
- [ ] SQL Server: thử tương tự với `CREATE DATABASE thu` / `GO` / `create table …` / `GO` / `use master` / `GO` / `drop database thu`.

---

## 4. Grid

### 4.1 Cuộn và hiển thị
- [ ] Chạy trên SQLite in memory:
  ```sql
  with recursive c(x) as (select 1 union all select x + 1 from c limit 2000000)
  select x, x * 2 as doubled, 'row ' || x as label from c
  ```
  **Đúng:** cuộn bằng chuột, thanh cuộn và phím đều mượt trong lúc dữ liệu còn đang tới; số cột nằm bên phải, chữ bên trái, NULL màu nhạt.
- [ ] Kéo mép phải tiêu đề cột để đổi độ rộng.

### 4.2 Chọn vùng
- [ ] Click một ô, rồi Shift+click ô khác.
  **Đúng:** vùng chữ nhật được tô; status bar ghi "N × M selected".
- [ ] Kéo chuột chọn vùng, kéo ra ngoài mép dưới.
  **Đúng:** grid tự cuộn.
- [ ] Click số dòng ở cột bên trái, kéo qua nhiều dòng.
  **Đúng:** chọn cả dòng.
- [ ] Ctrl+click tiêu đề cột, Ctrl+Shift+click tiêu đề cột khác.
  **Đúng:** chọn cả cột hoặc nhiều cột.
- [ ] Click ô góc trên trái, hoặc Ctrl+A.
  **Đúng:** chọn tất cả.
- [ ] Shift + mũi tên hoặc Page Down mở rộng vùng chọn; Esc thu về một ô.

### 4.3 Copy
- [ ] Chọn vùng 3×3, **Ctrl+C**, dán vào Excel.
  **Đúng:** đúng 3×3 ô; NULL thành ô trống; status bar báo "Copied 9 cells".
- [ ] **Ctrl+Shift+C** rồi dán.
  **Đúng:** có thêm dòng tên cột.
- [ ] Copy một ô chứa xuống dòng hoặc dấu `"` (ví dụ từ `select 'a"b' || chr(10) || 'c'` trên Postgres) rồi dán vào Excel.
  **Đúng:** nằm gọn trong một ô.

### 4.4 Row detail
- [ ] Bấm **Row detail** (hoặc Ctrl+Alt+I).
  **Đúng:** panel bên phải hiện mọi trường của dòng đang chọn với giá trị đầy đủ.
- [ ] Đi bằng ↑↓.
  **Đúng:** panel cập nhật theo dòng.
- [ ] Chọn ô JSON (ví dụ `select '{"a":[1,2]}'::jsonb`).
  **Đúng:** JSON được thụt lề cho dễ đọc; nút **Copy** chép giá trị đầy đủ.
- [ ] Click một trường trong panel.
  **Đúng:** con trỏ trong grid nhảy tới cột đó.

---

## 5. Sort

- [ ] Double-click bảng `orders`, bấm tiêu đề `total_amount` ba lần.
  **Đúng:** lần 1 ▲ tăng dần, lần 2 ▼ giảm dần, lần 3 bỏ sort; status bar ghi "sorted by the server"; đây là query chạy lại nên dữ liệu là mới nhất.
- [ ] Shift+click thêm cột `status`.
  **Đúng:** tiêu đề hiện ▲1 và ▲2.
- [ ] Chạy `select * from customers limit 5`, rồi sort theo `full_name`.
  **Đúng:** ra **5 tên đứng đầu bảng chữ cái của cả bảng**, không phải 5 dòng cũ xếp lại (ADR 0005).
- [ ] Chạy `select * from customers; select 1;` (Run all), rồi sort Result 1.
  **Đúng:** status bar ghi "sorted in Paboable"; không có query nào chạy lại.
- [ ] Đặt Limit **1,000**, chạy `select * from order_items` trong một script hai câu (để không chạy lại được), rồi sort.
  **Đúng:** status bar báo chỉ sort những dòng đã tải.
- [ ] Ctrl+click tiêu đề vẫn là chọn cột, không sort.

---

## 6. Row cap

- [ ] Chọn **Limit 1,000** trên toolbar, chạy `select * from order_items` (10.000 dòng).
  **Đúng:** dừng ở 1.000; status bar ghi "stopped at the row limit" và có nút **Fetch all**.
- [ ] Bấm **Fetch all**.
  **Đúng:** đủ 10.000 dòng.
- [ ] Đặt con trỏ trên câu đó, nhấn **Ctrl+Alt+Enter**.
  **Đúng:** chạy không giới hạn.
- [ ] Đóng rồi mở lại app.
  **Đúng:** Limit vẫn là lựa chọn cũ.

---

## 7. Giao diện chung

- [ ] Chuyển Windows sang **Dark mode** (Settings › Personalization › Colors).
  **Đúng:** app đổi theo: grid, editor và sidebar đều tối, chữ đọc rõ.
- [ ] Gõ tiếng Việt bằng Unikey/EVKey trong editor và ô Filter.
  **Đúng:** bỏ dấu bình thường.
- [ ] Kéo thanh chia giữa editor và kết quả, giữa sidebar và editor.

---

## Test tự động chạy trên server thật

Chạy từ gốc repo khi muốn kiểm lại phần driver và luồng chạy (thay mật khẩu):

```bash
set PABOABLE_POSTGRES_URL=postgres://postgres:<mật khẩu>@localhost:5432/postgres?sslmode=disable
set PABOABLE_MSSQL_URL=sqlserver://localhost
cargo test -p paboable-driver-postgres -- --ignored --test-threads=1
cargo test -p paboable-driver-mssql -- --ignored --test-threads=1
cargo test -p paboable-windows -- --ignored --test-threads=1
```

Test của `paboable-windows` có một test ghi rồi xóa một mục tạm trong Windows Credential Manager.
