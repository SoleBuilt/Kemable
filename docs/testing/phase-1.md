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
- [ ] Ở màn Welcome bấm **Open an in-memory database**, gõ `select 1 as one, 'xin chào' as greeting;`, nhấn **Ctrl+Enter**.
  **Đúng:** grid hiện 1 dòng, chữ tiếng Việt hiển thị đúng; combobox kết nối trên toolbar ghi tên kết nối và "SQLite 3.x".
- [ ] Bấm **Open file…** (Ctrl+O), chọn một file `.db` hoặc `.sqlite` có sẵn.
  **Đúng:** sidebar hiện schema `main` và các bảng; double-click một bảng thì bảng mở trên một tab riêng.

### 1.2 PostgreSQL
- [ ] Bấm **New connection…** (Ctrl+N), Engine chọn **PostgreSQL**, dán URL `postgres://postgres:<mật khẩu>@localhost:5432/postgres` vào ô URL.
  **Đúng:** Host, Port, User, Password tự điền; ô Database để trống cũng được.
- [ ] Nhập Name `Local PG`, giữ tick **Save this connection** và **Keep the password…**, bấm **Connect**.
  **Đúng:** kết nối thành công; sidebar hiện **tất cả database** (`demo_sales`, `postgres`), database đang dùng in đậm, có chấm xanh.
- [ ] Mở `demo_sales` → `public`.
  **Đúng:** 6 bảng `categories, customers, order_items, orders, payments, products` cùng các sequence.
- [ ] Thử sai mật khẩu.
  **Đúng:** hộp thoại báo "Access denied" và dòng lỗi của Postgres, không treo.

### 1.3 SQL Server
- [ ] **New connection…**, Engine **SQL Server**, Host `localhost`, Sign in with **Windows account**, Name `Local MSSQL`, bấm **Connect**.
  **Đúng:** sidebar hiện `master`, `model`, `msdb`, `tempdb` (và database của bạn nếu có); database người dùng đứng trước database hệ thống.
- [ ] Nếu có instance `SERVERAPP`: Host `localhost\SERVERAPP`, để trống Port.
  **Đúng:** kết nối qua SQL Server Browser.

### 1.4 Connection đã lưu
- [ ] Bấm combobox kết nối trên toolbar, chọn **Disconnect**.
  **Đúng:** về màn Welcome, không có sidebar.
- [ ] Nhấn **Ctrl+K**.
  **Đúng:** nhóm **Saved connections** có `Local PG` và `Local MSSQL`.
- [ ] Chọn `Local PG`, Enter.
  **Đúng:** kết nối ngay, không hỏi mật khẩu.
- [ ] Mở **Windows Credential Manager** → **Windows Credentials** → **Generic Credentials**.
  **Đúng:** có mục tên kiểu `connection/<id>/password.Paboable`.
- [ ] Mở `%APPDATA%\dev.paboable.app\connections.json` bằng Notepad.
  **Đúng:** có connection nhưng **không có mật khẩu**.
- [ ] Bấm combobox kết nối → **Edit this connection…**, đổi tên, bỏ tick **Keep the password…**, bấm **Save**.
  **Đúng:** mục trong Credential Manager biến mất; mở connection đó từ Ctrl+K thì app hỏi mật khẩu.
- [ ] Trong **New connection…**, combobox **Saved connection** chọn một connection, bấm **Delete…**, rồi xác nhận.
  **Đúng:** connection biến mất khỏi danh sách và khỏi Ctrl+K.

### 1.5 Test connection
- [ ] **New connection…**, PostgreSQL, điền host, user, mật khẩu đúng, bấm **Test**.
  **Đúng:** nút đổi thành "Testing…", rồi hiện khung xanh **Connected** với phiên bản server và "this login sees N databases". Không có kết nối nào mở (toolbar vẫn như cũ), không có gì được lưu.
- [ ] Sửa một chữ trong Host.
  **Đúng:** khung xanh biến mất (kết quả cũ không còn đúng).
- [ ] Đổi mật khẩu sai, bấm **Test**.
  **Đúng:** khung đỏ báo sai đăng nhập, kèm thông báo của server.
- [ ] Đổi Port thành `1`, bấm **Test**.
  **Đúng:** khung đỏ báo không kết nối được, trong vài giây.
- [ ] Chọn một Saved connection có giữ mật khẩu, để trống ô Password, bấm **Test**.
  **Đúng:** **Connected**: Test dùng mật khẩu trong Windows Credential Manager như Connect.
- [ ] Làm lại với SQL Server (Windows account) và Redis.
  **Đúng:** **Connected** với phiên bản; Redis không có số database.

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

- [ ] Bấm combobox database trên toolbar (cạnh kết nối), gõ để lọc, ↑↓ rồi Enter.
  **Đúng:** editor chuyển sang database đó; status bar ghi "in <database>"; nhánh của database đó trên sidebar mở ra.
- [ ] Double-click một database trên sidebar.
  **Đúng:** giống như trên.
- [ ] Double-click một bảng thuộc database khác database đang dùng.
  **Đúng:** bảng mở trên tab riêng, đọc từ database của nó.
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
- [ ] Click dòng 2, rồi **Ctrl+click** số dòng 5 và một ô ở dòng 9.
  **Đúng:** dòng 2, 5, 9 được tô cả dòng, các dòng giữa thì không; status bar ghi "3 rows selected".
- [ ] **Ctrl+click** lại dòng 5.
  **Đúng:** dòng 5 bỏ chọn, dòng 2 và 9 vẫn chọn; con trỏ ô nằm ở dòng 5.
- [ ] Sau đó **Shift+click** dòng 12.
  **Đúng:** thêm dòng 5 đến 12, dòng 2 vẫn chọn.
- [ ] Click thường một ô.
  **Đúng:** các dòng đã chọn bỏ hết, chỉ còn ô đó.

### 4.3 Copy
- [ ] Chọn vùng 3×3, **Ctrl+C**, dán vào Excel.
  **Đúng:** đúng 3×3 ô; NULL thành ô trống; status bar báo "Copied 9 cells".
- [ ] **Ctrl+Shift+C** rồi dán.
  **Đúng:** có thêm dòng tên cột.
- [ ] Ctrl+click chọn dòng 2, 5, 9 rồi **Ctrl+C**, dán vào Excel.
  **Đúng:** 3 dòng liền nhau theo thứ tự 2, 5, 9, đủ mọi cột.
- [ ] Trên tab bảng có khóa chính, Ctrl+click chọn 2 dòng cách nhau, nhấn **Delete**.
  **Đúng:** chỉ 2 dòng đó bị gạch để xóa; nhấn Delete lần nữa thì bỏ gạch.
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

- [ ] Chọn **Limit 1,000** ở bên phải thanh tab, chạy `select * from order_items` (10.000 dòng).
  **Đúng:** dừng ở 1.000; status bar ghi "Stopped at 1,000 rows" và có nút **Fetch all**.
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

## 8. Giao diện mới (design system, tab, bộ lọc kết quả)

Đối chiếu với prototype đã chốt: https://claude.ai/artifact/MGBUywSGe5pUwerFj3W8xz.

### 8.1 Welcome và Ctrl+K
- [ ] Mở app khi chưa có kết nối.
  **Đúng:** không có sidebar; giữa màn có **New connection…**, **Open file…**, **Open an in-memory database**; toolbar chỉ có ô Search.
- [ ] Nhấn **Ctrl+K**, gõ vài chữ, dùng ↑↓ rồi Enter; Esc để đóng.
  **Đúng:** bảng lệnh có nhóm Start, Saved connections, Settings; **Profile…** bị mờ (Later).

### 8.2 Tab query
- [ ] Kết nối, nhấn **Ctrl+T** hai lần.
  **Đúng:** có Query 1, Query 2, Query 3; tab mới chỉ có editor, chưa có vùng kết quả.
- [ ] Gõ và chạy ở Query 1, chuyển sang Query 2 rồi quay lại.
  **Đúng:** Query 1 giữ nguyên chữ, con trỏ, Ctrl+Z và kết quả.
- [ ] Đang chạy `select pg_sleep(10)` thì bấm sang tab khác.
  **Đúng:** status bar nhắc chờ hoặc Stop; không đổi tab.
- [ ] **Ctrl+W** đóng hết các tab query.
  **Đúng:** luôn còn một tab query trống.
- [ ] Mở nhiều tab đến khi tràn thanh.
  **Đúng:** thanh tab cuộn ngang (lăn chuột cũng cuộn), thanh cuộn mỏng; công cụ bên phải không bị đẩy ra ngoài.
- [ ] Chạy script có 3 câu SELECT, bấm lần lượt Result 1, 2, 3.
  **Đúng:** editor có khung nhẹ quanh câu tạo ra result đang xem.
- [ ] Kéo đường chia giữa editor và kết quả; double-click nó.
  **Đúng:** kéo được; double-click trả về chiều cao ban đầu.

### 8.3 Tab bảng
- [ ] Double-click bảng `order_items`.
  **Đúng:** mở tab riêng có icon bảng và số dòng; không có editor; pager nằm bên phải status bar.
- [ ] Bấm qua các trang, đổi số dòng mỗi trang (5 … 1,000); thử **Ctrl+[**, **Ctrl+]**, **Ctrl+Alt+↑/↓**.
  **Đúng:** đúng trang, đúng số dòng; đóng mở app thì vẫn nhớ số dòng mỗi trang.
- [ ] Bấm **Filter** (Ctrl+F), thêm hai điều kiện, đổi AND thành OR.
  **Đúng:** mọi dòng sau đổi theo; Apply lọc trên server, số dòng trên tab cập nhật.
- [ ] Sửa một ô, xem thanh tab.
  **Đúng:** bên phải thanh tab hiện "1 changed", **Discard**, **Preview & save**.
- [ ] Bấm số dòng ở cột trái.
  **Đúng:** chỉ chọn dòng, không đánh dấu xóa; nhấn **Delete** mới đánh dấu.
- [ ] Chuột phải lên bảng trong cây.
  **Đúng:** menu đủ các mục; Open, Open in New Tab, Copy Name, Refresh chạy được; các mục khác mờ với chữ Later.

### 8.4 Lọc kết quả query
- [ ] Chạy `select * from order_items` (Limit 1,000), bấm **Filter** trên thanh kết quả, thêm điều kiện `quantity > 3`, Apply.
  **Đúng:** lọc ngay, không chạy lại câu lệnh; status bar ghi "N of 1,000 rows held" và nhắc filter chỉ áp trên các dòng đã tải.
- [ ] Xem dải **To filter on the server**, bấm **Copy WHERE**, dán ra Notepad.
  **Đúng:** câu `WHERE "quantity" > 3` đúng cú pháp của engine.
- [ ] Sort một cột khi đang lọc.
  **Đúng:** sort ngay trên các dòng đang giữ, status bar ghi "sorted in Paboable".
- [ ] Chạy lại câu lệnh (Ctrl+Enter).
  **Đúng:** filter vẫn còn và áp vào kết quả mới. Bấm **Clear** mới bỏ.
- [ ] Ctrl+A, Ctrl+C rồi dán ra Excel.
  **Đúng:** chỉ có các dòng đang hiện.

### 8.5 Thanh tiêu đề
- [ ] Mở app.
  **Đúng:** không còn thanh tiêu đề của Windows; thanh trên cùng có logo bên trái, ô Search ở giữa, ba nút thu nhỏ, phóng to, đóng bên phải.
- [ ] Kéo vùng trống của thanh trên cùng; double-click vào đó.
  **Đúng:** kéo được cửa sổ; double-click phóng to rồi trả lại; nút giữa đổi giữa phóng to và khôi phục.
- [ ] Rê chuột lên nút đóng.
  **Đúng:** nền đỏ như Windows 11; bấm thì app đóng.
- [ ] Kéo cạnh và góc cửa sổ.
  **Đúng:** đổi được kích thước.

### 8.6 Ô nhập
- [ ] Bấm vào ô Filter objects, ô Value của filter, ô tìm trong Ctrl+K.
  **Đúng:** chỉ có con trỏ nhấp nháy, không có viền hay vòng sáng quanh ô.
- [ ] Ô Filter objects trên sidebar.
  **Đúng:** cao bằng các ô khác, cây đối tượng nằm ngay dưới, không có khoảng trống lớn.

### 8.7 Settings
- [ ] Nhấn **Ctrl+,**, đổi Theme Light / Dark / System.
  **Đúng:** đổi ngay cả grid và editor; **Cancel** trả lại theme cũ.
- [ ] Chọn **Compact · 24 px**, Save.
  **Đúng:** dòng grid thấp hơn; mở lại app vẫn nhớ.
- [ ] Bỏ tick "After CREATE DATABASE…", chạy lại script ở 3.1.
  **Đúng:** không chuyển database.
- [ ] Chọn Style **Orange**, chưa Save.
  **Đúng:** nút Save, gạch dưới tab đang mở, checkbox đã tick, viền con trỏ ô trong grid và con trỏ gõ trong editor đổi sang cam ngay; nền, chữ, đường kẻ không đổi. **Cancel** trả lại đen trắng.
- [ ] Chọn Orange, Save, đổi Theme Light rồi Dark.
  **Đúng:** Light: nút cam đậm chữ trắng. Dark: nút cam sáng chữ đen. Chữ trên nút đọc rõ ở cả hai. Mở lại app vẫn nhớ Orange.
- [ ] Trên Windows 11, bật **Mica background: On**, chưa Save.
  **Đúng:** toolbar và sidebar thấy màu nền desktop mờ phía sau (kéo cửa sổ qua hình nền có màu để thấy rõ); editor, grid, thanh tab, status bar và dialog vẫn đặc. **Cancel** trả lại nền đặc.
- [ ] Bật Mica, Save, đổi Theme Light / Dark / System, rồi đổi Windows sang Dark khi Theme là System.
  **Đúng:** lớp Mica sáng hoặc tối theo theme đang hiện; chữ trên toolbar và sidebar đọc rõ.
- [ ] Bật Mica, kéo đổi kích thước, maximize rồi restore cửa sổ.
  **Đúng:** không có chỗ nào trong editor hay grid lộ desktop; mở lại app vẫn nhớ Mica.
- [ ] Trên Windows 10 (nếu có), bật Mica.
  **Đúng:** dòng gợi ý đổi thành "This Windows cannot show Mica. It needs Windows 11."; app vẫn đặc, không lộ desktop.
- [ ] **Đo tốc độ** (cửa sổ giờ luôn trong suốt): chạy câu lệnh 2,000,000 dòng ở `.claude/rules/data-grid.md` trên SQLite in-memory, cuộn bằng chuột, thanh cuộn và bàn phím, một lần với Mica Off và một lần với Mica On.
  **Đúng:** DevTools › Performance (chuột phải › Inspect, bản dev) cho frame dưới 8 ms ở cả hai lần. Ghi số đo vào mô tả của merge.

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
