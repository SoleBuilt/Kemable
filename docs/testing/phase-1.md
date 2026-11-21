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
3. Mở DevTools khi cần xem lỗi JavaScript: bấm F12 hoặc Ctrl+Shift+I (chỉ có ở bản dev; app không còn menu chuột phải của trình duyệt).

---

## 1. Kết nối

### 1.1 SQLite
- [x] Ở màn Welcome bấm **Open an in-memory database**, gõ `select 1 as one, 'xin chào' as greeting;`, nhấn **Ctrl+Enter**.
  **Đúng:** grid hiện 1 dòng, chữ tiếng Việt hiển thị đúng; combobox kết nối trên toolbar ghi tên kết nối và "SQLite 3.x".
- [x] Bấm **Open file…** (Ctrl+O), chọn một file `.db` hoặc `.sqlite` có sẵn.
  **Đúng:** sidebar hiện schema `main` và các bảng; double-click một bảng thì bảng mở trên một tab riêng.

### 1.2 PostgreSQL
- [x] Bấm **New connection…** (Ctrl+N), Engine chọn **PostgreSQL**, dán URL `postgres://postgres:<mật khẩu>@localhost:5432/postgres` vào ô URL.
  **Đúng:** Host, Port, User, Password tự điền; ô Database để trống cũng được.
- [x] Nhập Name `Local PG`, giữ tick **Save this connection** và **Keep the password…**, bấm **Connect**.
  **Đúng:** kết nối thành công; sidebar hiện **tất cả database** (`demo_sales`, `postgres`), database đang dùng in đậm, có chấm xanh.
- [x] Mở `demo_sales` → `public`.
  **Đúng:** 6 bảng `categories, customers, order_items, orders, payments, products` cùng các sequence.
- [x] Thử sai mật khẩu.
  **Đúng:** hộp thoại báo "Access denied" và dòng lỗi của Postgres, không treo.
- [x] Điền Host, User, Password (hoặc chọn profile), bấm ô **Database** (ở dưới Encryption).
  **Đúng:** dòng "Reading the databases…" rồi hiện **Default** và các database login được thấy. Chọn một cái: ô hiện tên đó. Mở lại ô mà không đổi gì: không đọc lại.
- [x] Trong ô tìm của **Database**, gõ một tên không có trong danh sách, Enter.
  **Đúng:** dòng đầu là `Use "<tên>"`; chọn xong ô hiện tên đó. Đổi Password sai rồi mở lại ô: hiện dòng lỗi đăng nhập, vẫn gõ tên được.
- [x] Mở một bảng hơn 100 cột, bấm **Filter**, mở ô chọn cột.
  **Đúng:** danh sách cao tối đa khoảng 11 dòng rồi có thanh cuộn dọc; ô tìm luôn ở trên cùng; cột đang chọn nằm trong tầm nhìn khi mở.
- [x] Thu nhỏ cửa sổ cho form chạm mép dưới, mở ô **Database**.
  **Đúng:** danh sách mở lên trên (hoặc cuộn được bên trong), không bị cắt ở mép cửa sổ.

### 1.3 SQL Server
- [x] **New connection…**, Engine **SQL Server**, Host `localhost`, Sign in with **Windows account**, Name `Local MSSQL`, bấm **Connect**.
  **Đúng:** sidebar hiện `master`, `model`, `msdb`, `tempdb` (và database của bạn nếu có); database người dùng đứng trước database hệ thống.
- [x] Nếu có instance `SERVERAPP`: Host `localhost\SERVERAPP`, để trống Port.
  **Đúng:** kết nối qua SQL Server Browser.

### 1.4 Connection đã lưu
- [x] Bấm combobox kết nối trên toolbar, chọn **Disconnect**.
  **Đúng:** về màn Welcome, không có sidebar.
- [x] Nhấn **Ctrl+K**.
  **Đúng:** nhóm **Saved connections** có `Local PG` và `Local MSSQL`.
- [x] Chọn `Local PG`, Enter.
  **Đúng:** kết nối ngay, không hỏi mật khẩu.
- [x] Mở **Windows Credential Manager** → **Windows Credentials** → **Generic Credentials**.
  **Đúng:** có mục tên kiểu `connection/<id>/password.Kemable`.
- [x] Mở `connections.json` trong thư mục dữ liệu (Settings › Data › **Open**) bằng Notepad.
  **Đúng:** có connection nhưng **không có mật khẩu**.
  Kết quả 2026-11-21: connections.json (D:\Apps\kemable\data) có hai connection, `"password": ""`: không có mật khẩu.
- [x] Bấm combobox kết nối → **Edit this connection…**, đổi tên, bỏ tick **Keep the password…**, bấm **Save**.
  **Đúng:** mục trong Credential Manager biến mất; mở connection đó từ Ctrl+K thì app hỏi mật khẩu.
- [x] Trong **New connection…**, combobox **Saved connection** chọn một connection, bấm **Delete…**, rồi xác nhận.
  **Đúng:** connection biến mất khỏi danh sách và khỏi Ctrl+K.
- [x] Mở một connection đã lưu để sửa, ở **Colour** chọn màu đỏ (bấm chấm màu, hoặc Tab tới rồi dùng ← →), bấm **Save**. Nhấn **Ctrl+K**.
  **Đúng:** connection đó có chấm đỏ trước tên; connection chưa chọn màu có chấm xám. Ô **Saved connection** trong form cũng hiện chấm cùng màu. Kiểm tra cả theme sáng và tối.

### 1.5 Test connection
- [x] **New connection…**, PostgreSQL, điền host, user, mật khẩu đúng, bấm **Test**.
  **Đúng:** nút đổi thành "Testing…", rồi hiện khung xanh **Connected** với phiên bản server và "this login sees N databases". Không có kết nối nào mở (toolbar vẫn như cũ), không có gì được lưu.
- [x] Sửa một chữ trong Host.
  **Đúng:** khung xanh biến mất (kết quả cũ không còn đúng).
- [x] Đổi mật khẩu sai, bấm **Test**.
  **Đúng:** khung đỏ báo sai đăng nhập, kèm thông báo của server.
- [x] Đổi Port thành `1`, bấm **Test**.
  **Đúng:** khung đỏ báo không kết nối được, trong vài giây.
- [x] Chọn một Saved connection có giữ mật khẩu, để trống ô Password, bấm **Test**.
  **Đúng:** **Connected**: Test dùng mật khẩu trong Windows Credential Manager như Connect.
- [x] Làm lại với SQL Server (Windows account) và Redis.
  **Đúng:** **Connected** với phiên bản; Redis không có số database.

### 1.6 Credential profiles
- [x] Ctrl+K, gõ `profile`, chọn **Credential profiles…**.
  **Đúng:** hộp thoại rộng, bên trái danh sách (lần đầu: "No profiles yet"), bên phải trống "Make a profile to fill this in."
- [x] Bấm **New profile**, đặt Name `app-reader`, User và Password của một login PostgreSQL thật, bấm **Save** (hoặc Enter).
  **Đúng:** khung xanh **Saved**; danh sách ghi "app_reader · not used". Mở Windows Credential Manager › Windows Credentials: có `profile/<id>/password.Kemable`. Mở `profiles.json` trong thư mục dữ liệu (Settings › Data › **Open**): có tên và user, không có mật khẩu.
- [x] Tạo thêm profile tên `APP-READER` rồi Save.
  **Đúng:** khung đỏ báo tên đã có (không phân biệt hoa thường), không lưu.
  Kết quả 2026-11-21: Thử với profile tên `POSTGRES` khi đã có `postgres`: báo "a credential profile named POSTGRES already exists", không lưu. Tiêu đề lỗi lại là "The connection settings are not valid" (E11).
- [ ] **New connection…**, PostgreSQL, điền Host, ở **Credentials** chọn **From a profile**, chọn `app-reader`, bấm **Test**, rồi **Connect** với **Save this connection** bật.
  **Đúng:** ô User/Password biến mất, dòng gợi ý "Signs in as …"; ô "Keep the password…" ẩn; Test ra **Connected**; Connect mở được.
- [x] Mở lại **Credential profiles…**, chọn `app-reader`.
  **Đúng:** phần **Used by** có connection vừa lưu; danh sách ghi "used by 1".
  Kết quả 2026-11-21: Profile `postgres` ghi "used by 1", phần Used by có `postgresql:postgres@local`.
- [ ] Đổi Password của profile thành mật khẩu sai, Save, rồi mở connection đã lưu từ Ctrl+K.
  **Đúng:** báo sai đăng nhập (connection dùng mật khẩu mới của profile). Đổi lại mật khẩu đúng, Save: mở được, không hỏi mật khẩu.
- [x] Trong connect form, chọn **Type them here**, gõ User và Password, bấm **Save these as a profile…**.
  **Đúng:** form chuyển sang **From a profile** với profile mới tên theo user (thêm `-2` nếu trùng), khung xanh "Saved as the profile …".
- [x] Trong ô chọn Profile, chọn **Manage profiles…**.
  **Đúng:** Credential profiles mở đè lên connect form. Esc chỉ đóng Credential profiles, connect form vẫn còn; profile vừa sửa hiện đúng trong ô chọn.
- [ ] Trong Credential profiles, chọn `app-reader`, bấm **Delete** (hoặc phím Delete trong danh sách).
  **Đúng:** khung vàng "Delete app-reader? 1 connection uses it…" với Delete và Keep. Bấm Delete: profile biến mất, mục trong Credential Manager bị xoá; mở connection đã lưu thì hỏi mật khẩu.
- [x] Bấm **Duplicate** trên một profile.
  **Đúng:** thêm `<tên>-copy` cùng user, chưa lưu ("not saved"); phải gõ mật khẩu rồi Save.
  Kết quả 2026-11-21: Duplicate trên `postgres` thêm `postgres-copy`, ghi "not saved"; profiles.json không đổi khi chưa Save.
- [x] Kiểm tra cả hai hộp thoại ở theme sáng và tối.
  **Đúng:** chữ đọc rõ, danh sách bên trái có nền canvas, đường kẻ giữa hai cột.
- [x] Thu cửa sổ app thấp hơn connect form (kéo cạnh dưới lên), rồi mở connect form (New connection, chọn SQL Server).
  **Đúng:** form cao tối đa 80% cửa sổ, tiêu đề và hàng nút luôn thấy; chỉ phần giữa form cuộn, cửa sổ phía sau không có thanh cuộn. Thanh cuộn mỏng, chỉ hiện khi rê chuột vào form; khi nó hiện, các ô nhập không hẹp lại hay xê dịch.

---

## 2. Chạy query

- [x] Gõ 3 câu trên 3 dòng, đặt con trỏ ở câu giữa, nhấn **Ctrl+Enter**.
  **Đúng:** chỉ câu giữa chạy, câu đó được tô sáng rồi mờ dần; status bar ghi "Ran 1 of 3 statements".
- [x] Nhấn **Ctrl+Shift+Enter** (hoặc **F5**).
  **Đúng:** cả 3 câu chạy, có 3 tab Result.
- [x] Bôi đen một phần rồi Ctrl+Enter.
  **Đúng:** chỉ phần bôi đen chạy.
- [x] Postgres: `select pg_sleep(30);`, bấm **■ Stop** sau 1–2 giây.
  **Đúng:** dừng ngay; Messages báo "Stopped"; chạy `select 1` tiếp được.
- [x] SQL Server: `waitfor delay '00:00:30'`, bấm **■ Stop**.
  **Đúng:** như trên.
- [x] Gõ sai cú pháp, ví dụ `select * frm customers`.
  **Đúng:** tab Messages báo "The query failed" kèm lỗi của database.
- [x] Postgres: `do $$ begin raise notice 'xin chào'; end $$;`
  **Đúng:** Messages có dòng `xin chào`.
- [x] SQL Server: script có `GO`:
  ```sql
  select 1 as a
  GO
  select 2 as b
  ```
  **Đúng:** 2 tab Result.
- [x] F5 và Ctrl+R **không** tải lại cả trang (app không mất kết nối).

---

## 3. Database và sidebar

- [x] Bấm combobox database trên toolbar (cạnh kết nối), gõ để lọc, ↑↓ rồi Enter.
  **Đúng:** editor chuyển sang database đó; status bar ghi "in <database>"; nhánh của database đó trên sidebar mở ra.
- [x] Double-click một database trên sidebar.
  **Đúng:** giống như trên.
- [x] Double-click một bảng thuộc database khác database đang dùng.
  **Đúng:** bảng mở trên tab riêng, đọc từ database của nó.
- [x] Gõ vào ô **Filter** của sidebar.
  **Đúng:** chỉ còn các bảng có tên khớp.
- [x] Chạy `create table thu_refresh (x int);` rồi xem sidebar.
  **Đúng:** bảng mới tự hiện. Nhấn **Ctrl+R** hoặc nút ↻ cũng làm mới được.
- [x] Thu hẹp sidebar, bấm chọn một bảng có tên dài hơn sidebar, rồi cuộn ngang sidebar.
  **Đúng:** nền chọn phủ hết tên bảng cả khi cuộn ngang, không bị cắt ở mép sidebar.
- [x] Chọn một bảng, nhấn **Ctrl+C**, qua editor nhấn **Ctrl+V**. Làm lại với một database.
  **Đúng:** dán ra đúng tên; status bar ghi "Copied the name". **Ctrl+Enter** trên một bảng mở nó ở tab mới.
- [x] Bấm nút cây (bên trái nút ↻ trên header OBJECTS).
  **Đúng:** nút đổi sang trạng thái bấm; cây chỉ còn database đang dùng, bắt đầu từ các schema (không có dòng database). Đổi database ở toolbar: cây đổi theo. Bấm lại: về lại tất cả database. Mở lại app: nhớ lựa chọn.
- [x] Mở một database có hàng nghìn bảng (ví dụ database Business Central), cuộn nhanh xuống cuối và cuộn ngang.
  **Đúng:** cây hiện ngay khi mở, cuộn mượt, không có khoảng trống; dòng chọn vẫn phủ hết tên khi cuộn ngang.
- [x] SQL Server: một database có stored procedure và function.
  **Đúng:** có các thư mục **Stored Procedures**, **Table-valued Functions**, **Scalar-valued Functions** (đóng sẵn), mỗi thư mục đúng loại của nó.
  Kết quả 2026-11-21: Database thử có view, procedure, hai loại function: bốn thư mục Views, Stored Procedures, Table-valued Functions, Scalar-valued Functions đóng sẵn, mỗi thư mục đúng loại.
- [x] Chuột phải vào chỗ trống của sidebar, vào editor, vào status bar.
  **Đúng:** không hiện menu của trình duyệt (Back, Print, Inspect, Emoji…). Chuột phải vào bảng hoặc database trên cây vẫn hiện menu của app.

### 3.0b Menu chuột phải trên bảng (ADR 0017)
- [x] Chuột phải một bảng › **Show Structure**.
  **Đúng:** hộp thoại có danh sách cột (tên, kiểu, null, default, khoá) và nút **DDL**; Copy DDL và Open in editor dùng được.
- [x] **Show DDL** và **Copy DDL** trên một bảng có khoá chính, khoá ngoại, index, default, cột identity; trên một view; với SQL Server thêm procedure và function.
  **Đúng:** tab query mới có câu CREATE đầy đủ (SQL Server có các dòng `GO`). Chạy DDL đó dưới tên khác ở một database thử thì tạo được bảng giống hệt.
- [x] Chuột phải một view › **Edit View Definition**.
  **Đúng:** tab query có `CREATE OR REPLACE VIEW` (PostgreSQL) / `CREATE OR ALTER VIEW` (SQL Server) / `DROP VIEW … ; CREATE VIEW …` (SQLite); sửa rồi chạy được.
- [x] Chọn một bảng thử, nhấn **F2** (hoặc **Rename…**), gõ tên mới.
  **Đúng:** hộp thoại hiện câu lệnh sẽ chạy (SQL Server là `EXEC sp_rename …`), bấm Rename: cây làm mới với tên mới, status bar báo.
- [x] **Edit Comment…** trên bảng (PostgreSQL, SQL Server).
  **Đúng:** ô đã có comment hiện tại; lưu xong Show DDL thấy comment. SQLite không có mục này.
- [x] **Maintenance…** trên bảng.
  **Đúng:** danh sách việc đúng của engine (PostgreSQL: Vacuum, Analyze, Reindex…; SQL Server: Update statistics, Rebuild…; SQLite: Analyze, Reindex, Vacuum); câu lệnh hiện trước, Run chạy xong báo ở status bar.
- [x] Bảng thử: **Alt+Del** (Truncate), rồi **Del** (Drop).
  **Đúng:** cả hai hỏi bằng hộp thoại có câu lệnh và nút đỏ; Cancel không làm gì. PostgreSQL có ô CASCADE. SQLite Truncate là `DELETE FROM`.

### 3.0c Export, Import, Generate Rows (ADR 0018)
- [x] Chuột phải bảng › **Copy Qualified Name** trên SQL Server với bảng có khoảng trắng; trên PostgreSQL với bảng có chữ hoa; một bảng tên `order`.
  **Đúng:** `dbo.[CRONUS International …]`, `public."Orders"`, `"order"`; tên thường thì không có ngoặc. Dán vào editor chạy được.
  Kết quả 2026-11-21: SQL Server: `dbo.[Bang co khoang trang]`; PostgreSQL: `public."Orders"`, `public."order"`, tên thường `public.orders` không ngoặc; dán vào editor chạy được.
- [x] **Show Structure** › tab **Indexes**.
  **Đúng:** khoá chính đứng đầu, rồi các index theo tên; có cột, INCLUDE, unique, loại (CLUSTERED, btree…), điều kiện lọc.
  Kết quả 2026-11-21: PostgreSQL `edit_test`: Indexes có `edit_test_pkey`, cột `id`, "primary key · btree". Chưa thử bảng có index thường, INCLUDE hay điều kiện lọc.
- [x] Chuột phải bảng › **Export…**, CSV, giữ BOM, Export, chọn chỗ lưu. Mở bằng Excel.
  **Đúng:** status bar báo số dòng; Excel hiện đúng tiếng Việt; NULL là ô trống; ô có dấu phẩy, ngoặc kép, xuống dòng vẫn đúng.
  Kết quả 2026-11-21: PostgreSQL: file có BOM, dòng tên cột, CRLF, NULL là ô trống, tiếng Việt đúng (đọc bằng byte, không mở Excel).
- [x] Mở tab bảng, đặt filter, bấm **Export** trên thanh công cụ.
  **Đúng:** ô **Rows** chọn sẵn "Rows the filter keeps"; file chỉ có các dòng đó.
  Kết quả 2026-11-21: Tab `orders` lọc `status contains cancel`: ô Rows chọn sẵn "Rows the filter keeps (1)"; file có đúng 307 dòng, đều `Cancelled`.
- [x] Export một bảng lớn rồi bấm **Stop** giữa chừng.
  **Đúng:** hộp thoại đóng; không có file đích, không còn file `.part` trong thư mục.
  Kết quả 2026-11-21: View chậm `thu_cham` (60,000 dòng): bấm Stop khi "Writing 512 rows…": hộp thoại đóng, không có file đích, không còn `.part`. Bảng 3,000,000 dòng export xong trong khoảng 5 giây, status bar "Exported 3,000,000 rows to …".
- [ ] Export JSON và SQL INSERT.
  **Đúng:** JSON là mảng object, số là số; INSERT chạy lại được trên cùng engine.
  Kết quả 2026-11-21: **Lỗi (E3):** nội dung đúng (JSON là mảng object, số là số; INSERT đúng cú pháp), nhưng file JSON và SQL luôn bắt đầu bằng BOM UTF-8, không tắt được; `JSON.parse` báo "Unexpected token".
- [x] Chuột phải bảng › **Import…**, chọn file CSV (thử cả file `;` và file có BOM).
  **Đúng:** dấu phân cách đoán đúng; bảng "Field in the file → Column" khớp theo tên (không phân biệt hoa thường), field không có cột ghi "left out"; khung SQL hiện câu INSERT đầu; Import xong tab bảng hiện dòng mới, mã có số 0 đầu vẫn giữ.
  Kết quả 2026-11-21: Đã thử một phần: file dấu phẩy, khớp cột theo tên, khung SQL có câu INSERT đầu, Import xong có dòng mới (PostgreSQL, SQL Server, SQLite). Chưa thử file `;`, file có BOM, mã có số 0 đầu. **Lỗi (E5):** SQLite bỏ trường `id` của bảng có `INTEGER PRIMARY KEY` ("left out: no column of that name").
  Kết quả 2026-11-21: File `;` có BOM (`Ma;Ten;Gia`): đoán đúng `;`, BOM bị bỏ, trường có `;` trong nháy đọc đúng; `007` giữ số 0 đầu. (Lỗi E5 với `INTEGER PRIMARY KEY` của SQLite vẫn còn, ghi ở trên.)
- [x] Import với **Into a new table**, đổi tên bảng và kiểu một cột.
  **Đúng:** khung SQL có CREATE TABLE với kiểu đã đổi; xong cây có bảng mới.
  Kết quả 2026-11-21: Đổi tên `ma_hang` thành `hang_moi`, `Gia` từ `numeric` thành `numeric(10,2)`: khung SQL có CREATE TABLE với kiểu mới; Import xong cây có bảng mới, `Gia` là 1.50 và 2.00.
- [x] Import file lớn rồi **Stop** giữa chừng.
  **Đúng:** không dòng nào được thêm (rollback); bảng mới (nếu chọn) cũng không còn.
  Kết quả 2026-11-21: CSV 2,000,000 dòng vào bảng mới `thu_import_lon`, Stop lúc "Inserted 208,500 rows…": `to_regclass('thu_import_lon')` ra NULL, không có dòng nào và bảng mới cũng không còn.
- [x] Chuột phải bảng › **Generate Rows…**, 1000 dòng, bấm **New sample** vài lần, rồi Generate.
  **Đúng:** 5 dòng xem trước hợp kiểu cột (email ở cột email, ngày ở cột ngày, chuỗi không dài quá cột); cột identity/tính toán không có trong câu INSERT; khoá số nguyên đếm tiếp từ giá trị lớn nhất; xong có thêm đúng 1000 dòng.
  Kết quả 2026-11-21: `edit_test2`: id đếm tiếp từ 5 (lớn nhất là 4); New sample đổi 5 dòng xem trước (tên Việt và Anh, số, NULL); Generate 1,000: từ 3 lên 1,003 dòng, `max(id)` 1004, tên dài nhất 14 ký tự (cột 50).

### 3.1 Script tạo database
- [x] Postgres, Run all:
  ```sql
  CREATE DATABASE thu_tu_dong;
  create table bang_thu (id int);
  ```
  **Đúng:** Messages có "Switched to thu_tu_dong…"; `bang_thu` nằm trong `thu_tu_dong`, không nằm trong `postgres`.
- [x] Từ database `postgres`, chạy `DROP DATABASE thu_tu_dong;`.
  **Đúng:** xóa được, không báo "being accessed by other users".
- [x] SQL Server: thử tương tự với `CREATE DATABASE thu` / `GO` / `create table …` / `GO` / `use master` / `GO` / `drop database thu`.
  Kết quả 2026-11-21: Script `CREATE DATABASE thu` / GO / `create table` / GO / `use master` / GO / `drop database thu`: Messages ghi "Switched to thu", bảng nằm trong `thu`, drop được.

---

## 4. Grid

### 4.1 Cuộn và hiển thị
- [x] Chạy trên SQLite in memory:
  ```sql
  with recursive c(x) as (select 1 union all select x + 1 from c limit 2000000)
  select x, x * 2 as doubled, 'row ' || x as label from c
  ```
  **Đúng:** cuộn bằng chuột, thanh cuộn và phím đều mượt trong lúc dữ liệu còn đang tới; số cột nằm bên phải, chữ bên trái, NULL màu nhạt.
- [x] Kéo mép phải tiêu đề cột để đổi độ rộng.

### 4.2 Chọn vùng
- [x] Click một ô, rồi Shift+click ô khác.
  **Đúng:** vùng chữ nhật được tô; status bar ghi "N × M selected".
- [x] Kéo chuột chọn vùng, kéo ra ngoài mép dưới.
  **Đúng:** grid tự cuộn.
  Kết quả 2026-11-21: Kéo từ một ô xuống dưới mép grid: vùng chọn lan theo (21 × 3, rồi 23 × 3) và grid cuộn. Grid chỉ cuộn khi chuột còn di chuyển: giữ yên dưới mép thì dừng (E16).
- [x] Click số dòng ở cột bên trái, kéo qua nhiều dòng.
  **Đúng:** chọn cả dòng.
- [x] Ctrl+click tiêu đề cột, Ctrl+Shift+click tiêu đề cột khác.
  **Đúng:** chọn cả cột hoặc nhiều cột.
  Kết quả 2026-11-21: Ctrl+click `order_id`: "1,000 × 1 selected"; Ctrl+Shift+click `quantity`: "1,000 × 3 selected".
- [x] Click ô góc trên trái, hoặc Ctrl+A.
  **Đúng:** chọn tất cả.
- [x] Shift + mũi tên hoặc Page Down mở rộng vùng chọn; Esc thu về một ô.
  Kết quả 2026-11-21: Shift+↓ ↓ → ra "3 × 2 selected", Shift+Page Down ra "22 × 2 selected", Esc thu về một ô.
- [x] Click dòng 2, rồi **Ctrl+click** số dòng 5 và một ô ở dòng 9.
  **Đúng:** dòng 2, 5, 9 được tô cả dòng, các dòng giữa thì không; status bar ghi "3 rows selected".
- [x] **Ctrl+click** lại dòng 5.
  **Đúng:** dòng 5 bỏ chọn, dòng 2 và 9 vẫn chọn; con trỏ ô nằm ở dòng 5.
- [x] Sau đó **Shift+click** dòng 12.
  **Đúng:** thêm dòng 5 đến 12, dòng 2 vẫn chọn.
- [x] Click thường một ô.
  **Đúng:** các dòng đã chọn bỏ hết, chỉ còn ô đó.

### 4.3 Copy
- [x] Chọn vùng 3×3, **Ctrl+C**, dán vào Excel.
  **Đúng:** đúng 3×3 ô; NULL thành ô trống; status bar báo "Copied 9 cells".
- [x] **Ctrl+Shift+C** rồi dán.
  **Đúng:** có thêm dòng tên cột.
- [x] Ctrl+click chọn dòng 2, 5, 9 rồi **Ctrl+C**, dán vào Excel.
  **Đúng:** 3 dòng liền nhau theo thứ tự 2, 5, 9, đủ mọi cột.
- [x] Trên tab bảng có khóa chính, Ctrl+click chọn 2 dòng cách nhau, nhấn **Delete**.
  **Đúng:** chỉ 2 dòng đó bị gạch để xóa; nhấn Delete lần nữa thì bỏ gạch.
- [x] Copy một ô chứa xuống dòng hoặc dấu `"` (ví dụ từ `select 'a"b' || chr(10) || 'c'` trên Postgres) rồi dán vào Excel.
  **Đúng:** nằm gọn trong một ô.

### 4.4 Row detail
- [X] Bấm **Row detail** (hoặc Ctrl+Alt+I).
  **Đúng:** panel bên phải hiện mọi trường của dòng đang chọn với giá trị đầy đủ.
- [x] Đi bằng ↑↓.
  **Đúng:** panel cập nhật theo dòng.
- [x] Chọn ô JSON (ví dụ `select '{"a":[1,2]}'::jsonb`).
  **Đúng:** JSON được thụt lề cho dễ đọc; nút **Copy** chép giá trị đầy đủ.
  Kết quả 2026-11-21: PostgreSQL `'{"a":[1,2]}'::jsonb`: Row detail hiện JSON thụt lề, nút Copy chép `{"a": [1, 2]}`.
- [x] Click một trường trong panel.
  **Đúng:** con trỏ trong grid nhảy tới cột đó.

---

## 5. Sort

- [x] Double-click bảng `orders`, bấm tiêu đề `total_amount` ba lần.
  **Đúng:** lần 1 ▲ tăng dần, lần 2 ▼ giảm dần, lần 3 bỏ sort; status bar ghi "sorted by the server"; đây là query chạy lại nên dữ liệu là mới nhất.
- [x] Shift+click thêm cột `status`.
  **Đúng:** tiêu đề hiện ▲1 và ▲2.
  Kết quả 2026-11-21: Tab bảng `orders`: tiêu đề hiện ▼1 (total_amount) và ▲2 (status).
- [x] Chạy `select * from customers limit 5`, rồi sort theo `full_name`.
  **Đúng:** ra **5 tên đứng đầu bảng chữ cái của cả bảng**, không phải 5 dòng cũ xếp lại (ADR 0005).
- [x] Chạy `select * from customers; select 1;` (Run all), rồi sort Result 1.
  **Đúng:** status bar ghi "sorted in Kemable"; không có query nào chạy lại.
- [x] Đặt Limit **1,000**, chạy `select * from order_items` trong một script hai câu (để không chạy lại được), rồi sort.
  **Đúng:** status bar báo chỉ sort những dòng đã tải.
- [x] Ctrl+click tiêu đề vẫn là chọn cột, không sort.
- [x] **Sort trong Kemable những cột trước đây chậm** (thêm 2026-11-14, ADR 0005). Tắt Limit (hoặc bấm Fetch All), rồi trên PostgreSQL chạy script hai câu để kết quả không chạy lại được:
  ```sql
  select g, (g * 7919 % 1000003)::numeric(12, 2) / 100 as amount,
         decode(md5(g::text), 'hex') as hash,
         'INV-9' || lpad((g * 7919 % 1000003)::text, 39, '0') as invoice
  from generate_series(1, 1000000) g;
  select 1;
  ```
  Sort Result 1 lần lượt theo `amount`, `hash`, `invoice`, mỗi cột cả tăng lẫn giảm.
  **Đúng:** mỗi lần sort xong trong khoảng 1 giây hoặc nhanh hơn, không treo app. `amount` theo giá trị số (9.5 trước 10.25). `invoice` theo số (`INV-9…0099` trước `INV-9…0100`). `hash` theo byte. Shift+click thêm cột `g` vẫn đúng.
  Kết quả 2026-11-21: Câu trong tài liệu báo "integer out of range" ở dòng 270,848 (`g * 7919` tràn `integer`); chạy với `generate_series(1::bigint, 1000000)` (E8). 1,000,000 dòng: sort `amount` tăng xong trong khoảng 0,2 giây (0.01, 0.02…), giảm ra 10000.02 trước 10000.01; `hash` tăng bắt đầu `0x0000…`, giảm `0xffff…`; `invoice` theo số (`…1000002` trước `…1000001` khi giảm); Shift+click thêm `g` được. App không treo.
- [x] **Cột SQLite trộn số nguyên với số thực.** Mở **Open an in-memory database**, chạy:
  ```sql
  with recursive n(x) as (select 1 union all select x + 1 from n where x < 1000000)
  select x, case when x % 2 = 0 then x * 7919 % 1000003 else (x * 7919 % 1000003) + 0.5 end as mixed from n;
  select 1;
  ```
  rồi sort Result 1 theo `mixed`.
  **Đúng:** xong trong khoảng 1 giây; số nguyên và số thực xen nhau đúng giá trị (3, 3.5, 4).
  Kết quả 2026-11-21: 1,000,000 dòng (Fetch all), sort `mixed`: 1.5, 2.5, 3, 4, 5, 6.5… đúng giá trị, xong trong khoảng 1 giây.

---

## 6. Row cap

- [x] Chọn **Limit 1,000** ở bên phải thanh tab, chạy `select * from order_items` (10.000 dòng).
  **Đúng:** dừng ở 1.000; status bar ghi "Stopped at 1,000 rows" và có nút **Fetch all**.
- [x] Bấm **Fetch all**.
  **Đúng:** đủ 10.000 dòng.
- [x] Đặt con trỏ trên câu đó, nhấn **Ctrl+Alt+Enter**.
  **Đúng:** chạy không giới hạn.
- [x] Đóng rồi mở lại app.
  **Đúng:** Limit vẫn là lựa chọn cũ.

---

## 7. Giao diện chung

- [x] Chuyển Windows sang **Dark mode** (Settings › Personalization › Colors).
  **Đúng:** app đổi theo: grid, editor và sidebar đều tối, chữ đọc rõ.
- [x] Gõ tiếng Việt bằng Unikey/EVKey trong editor và ô Filter.
  **Đúng:** bỏ dấu bình thường.
- [x] Kéo thanh chia giữa editor và kết quả, giữa sidebar và editor.
- [x] Rê chuột lên các nút có tooltip ở: thanh tab trang (Copy SQL, Export của tab bảng), thanh Result (tab Result, Row detail), nút Refresh của sidebar, nút đóng và nút Test của connect form.
  **Đúng:** tooltip hiện đủ cả khung, không bị cắt thành một đường kẻ đen; tooltip của sidebar được tràn ra ngoài sidebar. Rời chuột thì tooltip tắt ngay.

---

## 8. Giao diện mới (design system, tab, bộ lọc kết quả)

Đối chiếu với prototype đã chốt: https://claude.ai/artifact/MGBUywSGe5pUwerFj3W8xz.

### 8.1 Welcome và Ctrl+K
- [x] Mở app khi chưa có kết nối.
  **Đúng:** không có sidebar; giữa màn có **New connection…**, **Open file…**, **Open an in-memory database**; toolbar chỉ có ô Search.
- [x] Nhấn **Ctrl+K**, gõ vài chữ, dùng ↑↓ rồi Enter; Esc để đóng.
  **Đúng:** bảng lệnh có nhóm Start, Saved connections, Settings; **Profile…** bị mờ (Later).
  Kết quả 2026-11-21: Bảng lệnh có Start, Saved connections, Recent files, Settings; ↑↓ và Enter mở đúng mục, Esc đóng. Không còn mục Profile… mờ: Credential profiles… đã có.

### 8.2 Tab query
- [x] Kết nối, nhấn **Ctrl+T** hai lần.
  **Đúng:** có Query 1, Query 2, Query 3; tab mới chỉ có editor, chưa có vùng kết quả.
  Kết quả 2026-11-21: PostgreSQL đang có Query 1–3: Ctrl+T hai lần mở Query 4, Query 5, trang mới chỉ có editor ("No result yet").
- [x] Gõ và chạy ở Query 1, chuyển sang Query 2 rồi quay lại.
  **Đúng:** Query 1 giữ nguyên chữ, con trỏ, Ctrl+Z và kết quả.
  Kết quả 2026-11-21: Chạy ở Query 5, gõ thêm, sang Query 4 rồi quay lại: chữ, Result 1 và Ctrl+Z (bỏ phần vừa gõ) còn nguyên.
- [x] Đang chạy `select pg_sleep(10)` thì bấm sang tab khác.
  **Đúng:** status bar nhắc chờ hoặc Stop; không đổi tab.
  Kết quả 2026-11-21: Không đổi tab; status bar: "Wait for the run to end, or stop it, before you switch pages".
- [x] **Ctrl+W** đóng hết các tab query.
  **Đúng:** luôn còn một tab query trống.
  Kết quả 2026-11-21: Ctrl+W tám lần (5 trang query và 1 trang bảng): còn một trang query trống.
- [x] Mở nhiều tab đến khi tràn thanh.
  **Đúng:** thanh tab cuộn ngang (lăn chuột cũng cuộn), thanh cuộn mỏng; công cụ bên phải không bị đẩy ra ngoài.
  Kết quả 2026-11-21: 15 tab: thanh tab tràn và cuộn ngang, lăn chuột cuộn được (`scrollLeft` 0 lên 300); Run, Save query và công cụ của trang vẫn trong cửa sổ.
- [x] Khi thanh tab đã tràn, double-click thêm vài bảng trong cây (mỗi bảng mở một tab), rồi Ctrl+Tab qua lại.
  **Đúng:** tab vừa mở hoặc vừa chọn luôn tự cuộn vào tầm nhìn, không nằm khuất ngoài mép. Thanh Result cũng vậy, và lăn chuột trên thanh Result cuộn ngang các tab.
  Kết quả 2026-11-21: Mở thêm `categories`, `customers` bằng Ctrl+Enter trên cây: tab vừa mở nằm trong tầm nhìn; Ctrl+Tab vòng về đầu, tab được chọn vẫn trong tầm nhìn. Chưa thử phần thanh Result.
- [x] Chạy script có 3 câu SELECT, bấm lần lượt Result 1, 2, 3.
  **Đúng:** editor có khung nhẹ quanh câu tạo ra result đang xem.
  Kết quả 2026-11-21: SQLite: chọn Result 2 thì editor có khung quanh dòng 2; tooltip "From statement 2 of the run" hiện đủ.
- [ ] Thu hẹp cửa sổ (còn khoảng 800px), chạy script có câu `select * from <bảng có tên dài>` rồi 6 câu `select 1;`, mở Result 1.
  **Đúng:** thanh Result chia hai phần: bên phải **Open <bảng> to edit**, Filter và nút Row detail luôn nằm trọn, tên bảng dài bị cắt bằng "…"; bên trái các tab Result cuộn ngang với thanh cuộn mỏng 4px, đường gạch dưới tab đang chọn vẫn thấy. Rê chuột lên một tab Result: tooltip "From statement … of the run" hiện đủ, không bị cắt. Mở Result 2 (không có Open … to edit): phần tab bên trái rộng ra chiếm chỗ trống. Cuộn phần tab sang phải hết rồi chạy lại: tab Result 1 tự cuộn về trong tầm nhìn.
- [ ] Trong khung đó, bôi đen một chữ bằng chuột, rồi double-click một chữ khác.
  **Đúng:** thấy rõ phần đang bôi đen ngay trong khung. (Các chỗ khác có cùng chữ cũng được tô nhạt, đó là đánh dấu chữ giống nhau, không phải chỗ đang chọn.)
- [x] SQL Server: hai câu `select` trên hai dòng, không có `GO` ở giữa, Run (Ctrl+Shift+Enter). Bấm qua lại Result 1 và Result 2.
  **Đúng:** mỗi Result có khung quanh đúng câu `select` của nó (cả batch vẫn gửi một lần như SSMS).
  Kết quả 2026-11-21: Result 1 có khung quanh câu đầu.
- [ ] Ở script đó, xoá `*` của câu đầu, để con trỏ sau `select ` và nhấn **Ctrl+Space**.
  **Đúng:** gợi ý chỉ có các cột của bảng ở câu đầu (bảng `$ext`), không lẫn cột của bảng ở câu dưới.
- [x] Ở script đó, mở Result 2, bấm header cột đầu để sort; rồi qua Result 1 sort cột đầu.
  **Đúng:** chỉ Result đang xem được sort, vẫn ở tab đó; status bar ghi "sorted in Kemable"; Result kia giữ nguyên, không có mũi tên sort.
  Kết quả 2026-11-21: Sort Result 2: chỉ Result 2 sort, "sorted in Kemable"; Result 1 giữ nguyên.
- [ ] SQL Server: `exec sp_who` rồi `select 1` (không `GO`), Run.
  **Đúng:** ra các Result nhưng không có khung (batch có `EXEC` không đọc được chắc chắn câu nào ra Result nào).
  Kết quả 2026-11-21: Ra hai Result (82 dòng và 1 dòng). Chưa kiểm phần không có khung.
- [x] Kéo đường chia giữa editor và kết quả; double-click nó.
  **Đúng:** kéo được; double-click trả về chiều cao ban đầu.
  Kết quả 2026-11-21: Kéo đường chia: chiều cao editor 188 lên 362; double-click về 188.
- [ ] Mở một connection đã lưu, gõ chữ khác nhau ở Query 1, 2, 3, để con trỏ giữa một dòng ở Query 2 và đang xem Query 2. Bấm nút đóng (×) của cửa sổ, mở lại app, mở lại connection đó.
  **Đúng:** có Query 1, 2, 3 với đúng chữ cũ; đang xem Query 2, con trỏ ở chỗ cũ. Chưa có kết quả nào (kết quả không được giữ).
  Kết quả 2026-11-21: Đã thử một phần: sau khi đóng và mở lại, chữ của các trang query và trang file trở lại. Trang đang xem lúc đóng là trang bảng (không được giữ), nên mở lại ở Query 1; chưa thử vị trí con trỏ.
- [ ] Gõ thêm vào Query 1 rồi đóng app ngay (trong 1 giây).
  **Đúng:** mở lại vẫn có chữ vừa gõ.
- [ ] Mở một connection khác (hoặc một file SQLite khác), rồi quay lại connection đầu.
  **Đúng:** mỗi connection có tab và chữ của riêng nó.
- [x] Mở một bảng thành tab bảng, đóng rồi mở lại app và connection.
  **Đúng:** chỉ tab query trở lại, tab bảng thì không.
  Kết quả 2026-11-21: Trang bảng `thu_so` không trở lại sau khi mở lại app; các trang query và file thì có.

### 8.2b File `.sql`
- [ ] Ở màn Welcome bấm **New query** (hoặc **Ctrl+T**), gõ một câu SQL, **Ctrl+T** mở thêm tab và gõ tiếp.
  **Đúng:** có editor và thanh tab, không có sidebar. Ctrl+S trên tab đó hỏi chỗ lưu file. Chọn connection trong Ctrl+K: các tab có chữ đi theo, đánh số tiếp sau các tab của connection, chạy được.
- [ ] Ngắt kết nối, **Ctrl+T**, rồi **Ctrl+W**.
  **Đúng:** về lại màn Welcome. Gõ chữ ở tab không connection rồi đóng mở lại app: chữ vẫn còn.
- [ ] Ở màn Welcome (chưa kết nối), nhấn **Ctrl+O**, chọn một file `.sql`.
  **Đúng:** mở thành tab mang tên file (rê chuột lên tab thấy đường dẫn), có editor, không có sidebar. Bấm **Run**: status bar nhắc mở connection và Ctrl+K mở ra.
- [ ] Chọn một connection trong Ctrl+K.
  **Đúng:** tab file đi theo sang connection đó, chạy được.
- [x] Gõ thêm chữ.
  **Đúng:** dấu × của tab đổi thành chấm tròn; rê chuột lên chấm thì lại thấy ×.
  Kết quả 2026-11-21: Tab `b.sql` có dấu chưa lưu sau khi gõ.
- [x] Nhấn **Ctrl+S**.
  **Đúng:** chấm biến mất, status bar ghi "Saved <tên file>". Mở file bằng Notepad: đúng nội dung; file UTF-16 hay UTF-8 có BOM vẫn giữ nguyên mã hoá (Notepad ghi ở góc dưới), xuống dòng vẫn CRLF.
  Kết quả 2026-11-21: "Saved b.sql"; file UTF-16 LE có BOM vẫn giữ mã hoá và CRLF (đọc bằng byte).
- [ ] Ở tab **Query n** (chưa gắn file), nhấn **Ctrl+S**.
  **Đúng:** hộp thoại Save As; lưu xong tab đổi tên thành tên file. **Ctrl+Shift+S** trên tab file: lưu ra tên khác, tab đổi theo.
- [x] Gõ thêm rồi bấm × của tab (hoặc Ctrl+W).
  **Đúng:** hỏi "Save changes to <tên>?" với **Save**, **Don't save**, **Cancel**; Cancel giữ tab, Don't save đóng mất chữ mới, Save lưu rồi đóng.
  Kết quả 2026-11-21: Hỏi "Save changes to a.sql?" với Don't save, Cancel, Save; Cancel giữ tab; Don't save đóng tab, file không đổi. Chưa thử Save trong hộp này.
- [ ] Gõ thêm (chưa lưu), đóng app bằng nút ×, mở lại app và connection.
  **Đúng:** không bị hỏi khi đóng app; mở lại thì tab file vẫn có chữ chưa lưu và chấm tròn. Nếu không sửa gì mà file bị đổi bằng Notepad, mở lại thấy nội dung mới của file.
- [x] **File bị sửa bên ngoài, tab chưa có thay đổi** (thêm 2026-11-14, ADR 0016). Mở một file `.sql` trong Kemable, không gõ gì. Mở chính file đó bằng Notepad, sửa một dòng, lưu, rồi bấm lại vào cửa sổ Kemable.
  **Đúng:** chữ trong tab đổi thành nội dung mới ngay, status bar ghi "Reloaded <tên file>: it changed outside Kemable", tab không có chấm tròn. **Ctrl+Z** trả lại chữ cũ (tab có chấm).
  Kết quả 2026-11-21: "Reloaded b.sql: it changed outside Kemable", tab không có dấu; Ctrl+Z trả chữ cũ (báo focus cửa sổ bằng sự kiện focus).
- [x] **File bị sửa bên ngoài, tab đang có thay đổi chưa lưu.** Gõ thêm vào tab (có chấm tròn), rồi sửa file bằng Notepad và lưu, bấm lại vào Kemable.
  **Đúng:** trên editor hiện dải vàng "<tên file> changed outside Kemable" với **Reload from disk** và **Keep my changes**; chữ trong tab chưa đổi.
  - Bấm **Reload from disk**: chữ thành nội dung của Notepad, dải biến mất, hết chấm tròn.
  - Làm lại, lần này bấm **Keep my changes**: chữ giữ nguyên, dải biến mất, vẫn có chấm tròn; **Ctrl+S** lưu đè lên file (mở Notepad lại thấy chữ của Kemable).
  Kết quả 2026-11-21: Dải "b.sql changed outside Kemable" với Reload from disk và Keep my changes; Keep rồi Ctrl+S ghi đè file; Reload from disk lấy nội dung mới.
- [x] **Ctrl+S không ghi đè bản chưa thấy.** Gõ thêm vào tab, sửa file bằng Notepad và lưu, rồi **không bấm ra ngoài**: nhấn Ctrl+S ngay trong Kemable (dùng Alt+Tab về Kemable rồi nhấn Ctrl+S cũng được).
  **Đúng:** không lưu; status bar ghi "<tên file> changed outside Kemable: reload it or keep your changes, then save" và dải vàng hiện ra.
  Kết quả 2026-11-21: Không lưu; status bar ghi "b.sql changed outside Kemable: reload it or keep your changes, then save"; file giữ nội dung bên ngoài.
- [x] **File bị xoá hoặc đổi tên bên ngoài.** Mở một file trong Kemable, rồi xoá (hoặc đổi tên) nó trong Explorer, bấm lại vào Kemable.
  **Đúng:** dải vàng "<tên file> is no longer on disk", tab có chấm tròn, chữ vẫn còn. Bấm **Save**: file được tạo lại đúng chỗ cũ, dải và chấm biến mất.
  Kết quả 2026-11-21: Xoá `a.sql`: dải "a.sql is no longer on disk… Ctrl+S writes it again"; Ctrl+S tạo lại file đúng chỗ.
- [ ] **Hai tab file, tab kia đang ẩn.** Mở hai file `.sql`, xem tab A, sửa file B bằng Notepad và lưu, bấm lại Kemable rồi chuyển sang tab B.
  **Đúng:** tab B đã có nội dung mới (nếu B không có thay đổi chưa lưu), hoặc hiện dải vàng (nếu có).
- [x] Ctrl+K, nhóm **Recent files**.
  **Đúng:** có các file vừa mở hoặc lưu, chọn là mở lại (nếu đã mở thì chuyển sang tab đó).
  Kết quả 2026-11-21: Có `a.sql`, `b.sql`, `postgres-demo-sales.sql`.
- [x] Mở một file không phải chữ (ví dụ đổi đuôi một file ảnh thành `.sql`).
  **Đúng:** không mở; status bar báo "This file is not UTF-8 or UTF-16 text".
  Kết quả 2026-11-21: Không mở tab; status bar: "This file is not UTF-8 or UTF-16 text: not UTF-8 or UTF-16 text: invalid utf-8 …" (câu bị lặp, E12).
- [ ] Ở tab bảng có chỉnh sửa, nhấn **Ctrl+S**.
  **Đúng:** vẫn mở Preview & save như trước (Ctrl+S chỉ lưu file ở tab query).

### 8.2b2 Thư mục `.sql` trong sidebar (thêm 2026-11-14, ADR 0016)
Chuẩn bị một thư mục, ví dụ `D:\scripts`, có vài file `.sql`, một thư mục con có file `.sql`, một thư mục con chỉ có file `.txt`, và một thư mục `.git` hoặc `node_modules` có file `.sql`.
- [x] Mở một connection. Ở sidebar mở pane **SQL files** (dưới Queries).
  **Đúng:** pane ghi "No folder linked" và có nút **Link folder…**.
  Kết quả 2026-11-21: Pane ghi "No folder linked" và có nút Link a folder.
- [x] Bấm **Link folder…**, chọn `D:\scripts`.
  **Đúng:** hiện dòng `scripts` với số file; bên dưới là thư mục con trước, rồi các file theo tên. Thư mục chỉ có `.txt`, thư mục `.git` và `node_modules` không hiện.
  Kết quả 2026-11-21: Dòng `scripts 4`, thư mục con `reports` trước rồi các file theo tên; `notes` (chỉ có .txt), `.git`, `node_modules` không hiện.
- [x] Bấm thư mục con để mở, bấm một file.
  **Đúng:** file mở thành tab query mang tên file, dòng của nó trong pane được tô. Bấm lại file đó thì chỉ chuyển sang tab đã mở, không mở tab thứ hai.
  Kết quả 2026-11-21: Bấm `a.sql` mở thành tab mang tên file. Chưa kiểm dòng được tô và bấm lần hai.
- [x] Gõ một phần tên file vào ô **Find a file**.
  **Đúng:** chỉ còn các file có chữ đó, kèm đường dẫn con (ví dụ `reports/sales.sql`).
  Kết quả 2026-11-21: Gõ `sal`: còn `reports/sales.sql`.
- [x] Tạo một file `.sql` mới trong thư mục bằng Explorer, rồi bấm lại vào cửa sổ Kemable.
  **Đúng:** file mới hiện trong pane (không cần Refresh). Save As một tab vào thư mục đó cũng làm file hiện ra.
  Kết quả 2026-11-21: File `moi.sql` tạo bên ngoài hiện sau khi cửa sổ nhận focus (`scripts 5`). Chưa thử Save As vào thư mục.
- [ ] Chuột phải dòng `scripts`: có **Refresh**, **Link another folder…**, **Remove from the sidebar**. Chọn Remove.
  **Đúng:** thư mục biến mất khỏi pane, status bar ghi file vẫn còn trên đĩa; file trên đĩa không bị đụng tới.
  Kết quả 2026-11-21: **Lỗi (E6):** menu có đủ ba mục, nhưng Refresh và Remove from the sidebar không làm gì: lỗi JS "Cannot read properties of null (reading 'folder')"; thư mục vẫn còn, cả sau khi mở lại app.
- [ ] Link lại thư mục, đóng app, mở lại và mở một connection khác.
  **Đúng:** thư mục vẫn còn trong pane SQL files (dùng chung cho mọi connection).
- [x] Ctrl+K, gõ `link`.
  **Đúng:** có lệnh **Link a folder of .sql files…**.
  Kết quả 2026-11-21: Có "Link a folder of .sql files…".
- [ ] Link một thư mục rất lớn (ví dụ `C:\Windows`).
  **Đúng:** app không treo; pane hiện tối đa 5,000 file, kèm dòng "Only the first 5,000 files are listed" khi có nhiều hơn. Sau đó Remove nó.

### 8.2c Open <table> to edit
- [x] Chạy `select * from <bảng> where City = 'Miami'` (đổi tên bảng, cột cho đúng database).
  **Đúng:** thanh kết quả có nút **Open <bảng> to edit**. Bấm: mở tab bảng, hàng Filter mở sẵn với điều kiện `City = Miami`, chỉ các dòng đó hiện ra, sửa được như tab bảng.
  Kết quả 2026-11-21: SQL Server: nút Open hang to edit mở tab bảng, Filter `ten is Miami`, 2 dòng.
- [x] Chạy `select * from <bảng> where a = 1 or b like 'x%'`.
  **Đúng:** bộ lọc có hai điều kiện, nối bằng **OR**, điều kiện thứ hai là "starts with x".
  Kết quả 2026-11-21: `id = 1 or ten like 'Ha%'`: hai điều kiện, nối bằng OR, điều kiện sau là "starts with Ha".
- [x] Chạy `select * from <bảng> where upper(City) = 'MIAMI'` rồi bấm nút.
  **Đúng:** mở cả bảng không lọc; status bar nói WHERE không chuyển được sang bộ lọc.
  Kết quả 2026-11-21: Mở cả bảng; status bar: "Opened the whole table: the statement's WHERE is more than the filter row holds".
- [x] Chạy câu có JOIN, GROUP BY, DISTINCT hoặc UNION.
  **Đúng:** không có nút.
  Kết quả 2026-11-21: Câu có JOIN: không có nút. Chưa thử GROUP BY, DISTINCT, UNION.
- [ ] SQL Server: hai câu `select` không có `GO`, câu đầu có WHERE. Bấm nút ở Result 1 rồi ở Result 2.
  **Đúng:** mỗi nút mở đúng bảng của câu đó; chỉ Result 1 mang theo bộ lọc.

### 8.3 Tab bảng
- [x] Double-click bảng `order_items`.
  **Đúng:** mở tab riêng có icon bảng và số dòng; không có editor; pager nằm bên phải status bar.
  Kết quả 2026-11-21: PostgreSQL `orders`: tab riêng có icon bảng và số dòng, không có editor, pager bên phải status bar.
- [x] Bấm qua các trang, đổi số dòng mỗi trang (5 … 1,000); thử **Ctrl+[**, **Ctrl+]**, **Ctrl+Alt+↑/↓**.
  **Đúng:** đúng trang, đúng số dòng; đóng mở app thì vẫn nhớ số dòng mỗi trang.
  Kết quả 2026-11-21: Ctrl+] / Ctrl+[, nút First/Last, đổi 100 dòng mỗi trang (30 trang). Chưa thử Ctrl+Alt+↑/↓ và nhớ số dòng sau khi mở lại.
- [x] Bấm **Filter** (Ctrl+F), thêm hai điều kiện, đổi AND thành OR.
  **Đúng:** mọi dòng sau đổi theo; Apply lọc trên server, số dòng trên tab cập nhật.
  Kết quả 2026-11-21: `status contains cancel` AND `customer_id is 4`: 11 dòng; đổi OR: 419; khớp với câu đếm SQL.
- [x] Sửa một ô, xem thanh tab.
  **Đúng:** bên phải thanh tab hiện "1 changed", **Discard**, **Preview & save**.
  Kết quả 2026-11-21: Thanh tab có "1 changed", Discard, Preview & save.
- [x] Bấm số dòng ở cột trái.
  **Đúng:** chỉ chọn dòng, không đánh dấu xóa; nhấn **Delete** mới đánh dấu.
  Kết quả 2026-11-21: SQLite `thu_so2`: bấm số dòng 2 chỉ chọn dòng ("1 × 4 selected", không có gì để lưu); nhấn Delete mới đánh dấu ("1 to delete", dòng gạch đỏ).
- [x] Chuột phải lên bảng trong cây.
  **Đúng:** menu đủ các mục; Open, Open in New Tab, Copy Name, Refresh chạy được; các mục khác mờ với chữ Later.
  Kết quả 2026-11-21: Menu đủ mục (Open, Open in New Tab, Show Structure, DDL, Copy Name, Export, Import, Generate Rows, Rename, Comment, Maintenance, Truncate, Drop); chỉ Transfer To… và Copy To… còn Later.

### 8.4 Lọc kết quả query
- [x] Chạy `select * from order_items` (Limit 1,000), bấm **Filter** trên thanh kết quả, thêm điều kiện `quantity > 3`, Apply.
  **Đúng:** lọc ngay, không chạy lại câu lệnh; status bar ghi "N of 1,000 rows held" và nhắc filter chỉ áp trên các dòng đã tải.
  Kết quả 2026-11-21: PostgreSQL: "104 of 1,000 rows held" và "the filter covers only the rows loaded", không chạy lại câu.
- [x] Xem dải **To filter on the server**, bấm **Copy WHERE**, dán ra Notepad.
  **Đúng:** câu `WHERE "quantity" > 3` đúng cú pháp của engine.
  Kết quả 2026-11-21: SQLite: `WHERE "n" > 3`. (PostgreSQL ghi `"quantity" > '3'`, số trong nháy, vẫn chạy được.)
- [x] Sort một cột khi đang lọc.
  **Đúng:** sort ngay trên các dòng đang giữ, status bar ghi "sorted in Kemable".
  Kết quả 2026-11-21: "sorted in Kemable" ngay trên các dòng đang giữ.
- [ ] Chạy lại câu lệnh (Ctrl+Enter).
  **Đúng:** filter vẫn còn và áp vào kết quả mới. Bấm **Clear** mới bỏ.
  Kết quả 2026-11-21: **Lỗi (E2):** chạy lại thì Result 1 trống, "0 of 0 rows held", "Stopped at 0 rows"; hàng Filter về mặc định trong khi dải WHERE vẫn còn.
- [x] Ctrl+A, Ctrl+C rồi dán ra Excel.
  **Đúng:** chỉ có các dòng đang hiện.
  Kết quả 2026-11-21: Clipboard có đúng 104 dòng đang hiện, tất cả `quantity > 3` (không mở Excel).

### 8.5 Thanh tiêu đề
- [x] Mở app.
  **Đúng:** không còn thanh tiêu đề của Windows; thanh trên cùng có logo bên trái, ô Search ở giữa, ba nút thu nhỏ, phóng to, đóng bên phải.
  Kết quả 2026-11-21: Không có thanh tiêu đề Windows; logo bên trái, ô Search giữa, ba nút bên phải.
- [ ] Kéo vùng trống của thanh trên cùng; double-click vào đó.
  **Đúng:** kéo được cửa sổ; double-click phóng to rồi trả lại; nút giữa đổi giữa phóng to và khôi phục.
- [ ] Rê chuột lên nút đóng.
  **Đúng:** nền đỏ như Windows 11; bấm thì app đóng.
- [ ] Kéo cạnh và góc cửa sổ.
  **Đúng:** đổi được kích thước.
- [ ] Kéo cửa sổ sang chỗ khác và đổi kích thước, bấm nút đóng, mở lại app.
  **Đúng:** cửa sổ mở đúng chỗ và đúng kích thước lúc đóng, không chớp ở chỗ mặc định trước. Phóng to rồi đóng, mở lại: app mở phóng to; bấm khôi phục thì về đúng chỗ và kích thước trước khi phóng to.
- [ ] (Nếu có hai màn hình) Để cửa sổ ở màn hình phụ, đóng app, rút màn hình phụ, mở lại.
  **Đúng:** cửa sổ hiện trên màn hình còn lại, không nằm ngoài tầm nhìn.

### 8.6 Ô nhập
- [ ] Bấm vào ô Filter objects, ô Value của filter, ô tìm trong Ctrl+K.
  **Đúng:** chỉ có con trỏ nhấp nháy, không có viền hay vòng sáng quanh ô.
- [x] Ô Filter objects trên sidebar.
  **Đúng:** cao bằng các ô khác, cây đối tượng nằm ngay dưới, không có khoảng trống lớn.
  Đã kiểm tra trên In memory (SQLite 3.54.1): ô lọc cao bằng các ô tìm khác; cây Objects nằm ngay dưới, không có khoảng trống lớn.
  Mục ô nhập phía trên vẫn chưa đạt: đã bấm ô Filter objects và ô tìm trong Ctrl+K, nhưng ảnh chụp tĩnh không xác nhận được con trỏ nhấp nháy hay viền focus; ô Value chưa có grid để thử.

### 8.7 Settings
- [x] Nhấn **Ctrl+=** hai lần (hoặc Ctrl và phím + trên bàn số), rồi **Ctrl+-**, rồi **Ctrl+0**.
  **Đúng:** cả cửa sổ to lên 120 %, 144 %, nhỏ lại 120 %, rồi về 100 %; status bar báo "Zoom 144 %"… Chữ trong grid vẫn nét ở mọi mức. Đóng mở lại app: giữ mức zoom. Settings › **Zoom** đổi được và xem ngay; Cancel trả về mức cũ.
  Đã thử Ctrl+= hai lần, Ctrl+-, Ctrl+0: kích thước giao diện lần lượt đổi tương ứng 120 %, 144 %, 120 % và 100 %. Trong Settings cũng đổi được 100 % → 120 %; Cancel trả giao diện về 100 %. Chưa xác nhận status bar, độ nét của grid hay mức zoom sau khi mở lại app.
  Kết quả 2026-11-21: Status bar ghi "Zoom 144 %", "Zoom 120 %", "Zoom 100 %"; mở lại app vẫn 120 % (`zoomLevel: 1`).
- [x] Nhấn **Ctrl+,**, đổi Theme Light / Dark / System.
  **Đúng:** đổi ngay cả grid và editor; **Cancel** trả lại theme cũ.
  Đã đổi System → Light → Dark → System: giao diện sáng ở Light và tối ở Dark/System theo Windows. Đổi System → Light rồi Cancel đóng Settings; lần mở Settings kế tiếp vẫn là System. Editor đổi giao diện theo theme; grid không hiện do query còn `Running…`, nên chưa xác nhận được phần grid.
  Kết quả 2026-11-21: Light/Dark đổi ngay nền app, grid và editor; Cancel trả lại theme cũ.
- [x] Chọn **Compact · 24 px**, Save.
  **Đúng:** dòng grid thấp hơn; mở lại app vẫn nhớ.
  Kết quả 2026-11-21: `compactRows: true` lưu vào ui.json, còn sau khi mở lại app (đã trả về Default sau khi thử).
- [x] Bỏ tick "After CREATE DATABASE…", chạy lại script ở 3.1.
  **Đúng:** không chuyển database.
  Kết quả 2026-11-21: PostgreSQL: `CREATE DATABASE thu_khong_theo; create table …; select current_database()` ra `postgres`: không chuyển database. Đã dọn.
- [x] Chọn Style **Orange**, chưa Save.
  **Đúng:** nút Save, gạch dưới tab đang mở, checkbox đã tick, viền con trỏ ô trong grid và con trỏ gõ trong editor đổi sang cam ngay; nền, chữ, đường kẻ không đổi. **Cancel** trả lại đen trắng.
  Đã đổi Black and white → Orange chưa lưu: nút Run/Save và checkbox chuyển cam ngay. Cancel đóng Settings và giao diện trở về Black and white. Không có grid kết quả vì query còn `Running…`, nên chưa xác nhận màu con trỏ ô, con trỏ gõ hay gạch dưới tab.
  Kết quả 2026-11-21: Light + Orange: nút Run, Apply, gạch dưới tab và viền con trỏ ô chuyển cam ngay; Cancel trả lại đen trắng.
- [ ] Chọn Orange, Save, đổi Theme Light rồi Dark.
  **Đúng:** Light: nút cam đậm chữ trắng. Dark: nút cam sáng chữ đen. Chữ trên nút đọc rõ ở cả hai. Mở lại app vẫn nhớ Orange.
- [ ] Trên Windows 11, bật **Mica background: On**, chưa Save.
  **Đúng:** toolbar và sidebar thấy màu nền desktop mờ phía sau (kéo cửa sổ qua hình nền có màu để thấy rõ); editor, grid, thanh tab, status bar và dialog vẫn đặc. **Cancel** trả lại nền đặc.
  Đã bật Mica trong Settings: lựa chọn **On** được chọn. Ảnh chụp không cho thấy rõ nền desktop xuyên mờ; chưa kéo cửa sổ qua nền có màu nên chưa xác nhận hiệu ứng. Đã bấm **Cancel**, không lưu thay đổi.
- [ ] Bật Mica, Save, đổi Theme Light / Dark / System, rồi đổi Windows sang Dark khi Theme là System.
  **Đúng:** lớp Mica sáng hoặc tối theo theme đang hiện; chữ trên toolbar và sidebar đọc rõ.
- [ ] Bật Mica, kéo đổi kích thước, maximize rồi restore cửa sổ.
  **Đúng:** không có chỗ nào trong editor hay grid lộ desktop; mở lại app vẫn nhớ Mica.
- [ ] Trên Windows 10 (nếu có), bật Mica.
  **Đúng:** dòng gợi ý đổi thành "This Windows cannot show Mica. It needs Windows 11."; app vẫn đặc, không lộ desktop.
- [ ] **Đo tốc độ** (cửa sổ giờ luôn trong suốt): chạy câu lệnh 2,000,000 dòng ở `.claude/rules/data-grid.md` trên SQLite in-memory, cuộn bằng chuột, thanh cuộn và bàn phím, một lần với Mica Off và một lần với Mica On.
  **Đúng:** DevTools › Performance (F12, bản dev) cho frame dưới 8 ms ở cả hai lần. Ghi số đo vào mô tả của merge. (Đã đo tự động ngày 2026-11-11 trên một máy: không khác nhau; số đo ở ADR 0012, Consequences. Vẫn nên thử bằng tay trên máy khác, nhất là màn hình 120 Hz hoặc 4K.)

### 8.8 Cài đặt và thư mục dữ liệu (ADR 0019)
Dùng file cài `Kemable_<version>_x64-setup.exe` (`pnpm tauri build`, ở `target\release\bundle\nsis\`).
- [ ] Chạy file cài, bấm Next ở trang chào.
  **Đúng:** không có trang điều khoản; app hỏi điều khoản khi mở lần đầu.
- [ ] (Tài khoản admin) Trang tiếp theo hỏi cài cho mình hay cho mọi người. Tài khoản thường không thấy trang này.
- [ ] Ở trang **Choose Install Location**, bấm Browse, chọn một thư mục trên ổ D (ví dụ `D:\Apps\Kemable`), Next.
  **Đúng:** cài xong, trang **Completing Kemable Setup** có ô Run Kemable và Create desktop shortcut; bấm Finish.
- [ ] Mở app, vào Settings.
  **Đúng:** mục **Data** ghi "Beside the app, where it was installed: D:\Apps\Kemable\data…"; bấm **Open** mở đúng thư mục đó trong Explorer, có `connections.json`, `ui.json`, `pages\`, `webview\`.
  Kết quả 2026-11-21: Mục Data ghi "Beside the app, where it was installed: D:\Apps\kemable\data". Chưa bấm Open.
- [ ] Mở app lần đầu sau khi cài (hoặc một bản copy, hoặc `pnpm tauri dev` với thư mục dữ liệu mới).
  **Đúng:** **không** có hộp thoại **Terms of Service and Privacy Policy** (tạm ẩn cho tới khi Kemable có điều khoản riêng, ADR 0023); vào thẳng màn Welcome.
- [ ] Settings › **About** (ADR 0023).
  **Đúng:** dòng đầu ghi `Kemable 0.1.0` (đúng version trong `tauri.conf.json`) và câu về AGPL. **License** mở trang LICENSE trên GitHub; **Source code › Open** mở `github.com/SoleBuilt/Kemable`; **Third-party notices › Open** mở `THIRD-PARTY-NOTICES.md` bằng Notepad, có các mục Rust crates, npm packages, Lucide icons.
  Kết quả 2026-11-21: Dòng đầu ghi `Kemable 0.1.0` và câu về AGPL. Chưa bấm License, Source code, Third-party notices.
- [ ] Windows Settings › Apps › Installed apps, tìm Kemable.
  **Đúng:** Publisher ghi **SoleBuilt** (không phải `app`).
- [ ] Máy đã từng dùng bản cũ (có `%APPDATA%\kemable.app`): mở app lần đầu sau khi cài.
  **Đúng:** connection đã lưu và lịch sử query vẫn còn. Mở lại app lần nữa: vẫn còn.
- [ ] Gõ vào một tab query, đóng app ngay bằng **Alt+F4** (không chờ), mở lại.
  **Đúng:** chữ vừa gõ vẫn còn.
- [ ] Kéo rộng sidebar và kéo cao editor, đóng app, mở lại.
  **Đúng:** sidebar và editor giữ đúng kích thước vừa kéo.
- [ ] Gỡ cài đặt (Settings của Windows › Apps), **không** tick "Delete the application data".
  **Đúng:** thư mục cài chỉ còn `data`. Cài lại vào đúng thư mục đó: mọi thứ trở lại như trước.
- [ ] Gỡ cài đặt, **có** tick "Delete the application data".
  **Đúng:** thư mục cài biến mất cùng `data`.

---

## Test tự động chạy trên server thật

Chạy từ gốc repo khi muốn kiểm lại phần driver và luồng chạy (thay mật khẩu):

```bash
set KEMABLE_POSTGRES_URL=postgres://postgres:<mật khẩu>@localhost:5432/postgres?sslmode=disable
set KEMABLE_MSSQL_URL=sqlserver://localhost
cargo test -p kemable-driver-postgres -- --ignored --test-threads=1
cargo test -p kemable-driver-mssql -- --ignored --test-threads=1
cargo test -p kemable-windows -- --ignored --test-threads=1
```

Test của `kemable-windows` có một test ghi rồi xóa một mục tạm trong Windows Credential Manager.
