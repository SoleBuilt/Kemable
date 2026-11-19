# Kiểm thử thủ công: các sửa lỗi trước bản preview

Các bước người bấm mới kiểm được cho từng mục của `docs/preview-audit.md`, một phần cho mỗi mục, cùng mã và cùng thứ tự với bản audit. Phần chuẩn bị (mục 0) giống `docs/testing/phase-1.md`.

**Cách ghi kết quả:** đánh dấu `[x]` khi đúng. Khi sai, ghi ngay dưới dòng đó: đã làm gì, thấy gì, mong đợi gì. Sau đó nhờ agent dùng skill `fix-issue`.

---

## A1. SQL Server: `USE` giữa batch không kéo database đầu tiên đi theo

Kemable giữ một phiên cho mỗi database đang dùng. Một batch có `USE x` ở bất kỳ chỗ nào làm phiên đó sang `x`; driver đưa phiên về database của nó ngay sau batch, và lần chạy đi tiếp trên phiên của `x`.

- [ ] Kết nối SQL Server, chọn database `master` ở thanh công cụ. Mở tab query, chạy cả khối sau (một batch, không có `GO`):
  ```sql
  select 1;
  use tempdb;
  select db_name();
  ```
  **Đúng:** kết quả thứ hai là `tempdb`; database ở thanh công cụ chuyển sang `tempdb`; tab Messages ghi đã chuyển database.
- [ ] Chọn lại `master` ở thanh công cụ, chạy `select db_name();`.
  **Đúng:** ra `master`, không phải `tempdb`.
- [ ] Trong cây, mở `master`, mở một bảng hệ thống bất kỳ (ví dụ `dbo.spt_values`) bằng tab bảng.
  **Đúng:** tab bảng đọc dữ liệu của `master`; không có lỗi "Invalid object name".
- [ ] Tạo hai database thử: `create database KemableA; create database KemableB;` (chạy từng câu). Chọn `KemableA`, chạy `create table t (id int primary key, ten nvarchar(20)); insert into t values (1, N'a');`. Chọn `KemableB`, chạy `create table t (id int primary key, ten nvarchar(20)); insert into t values (1, N'b');`.
- [ ] Chọn `KemableA`, chạy `select 1; use KemableB;`. Chọn lại `KemableA` ở thanh công cụ. Trong cây mở `KemableA > dbo.t`, sửa ô `ten` của dòng 1 thành `sua`, lưu.
  **Đúng:** ở `KemableA` dòng 1 thành `sua`; chạy `select ten from KemableB.dbo.t` ra `b` (không bị sửa).
- [ ] Dọn dẹp: chọn `master`, chạy `drop database KemableA;` rồi `drop database KemableB;`.
  **Đúng:** cả hai câu chạy được, không bị từ chối vì "database is in use".

---

## A2. Export CSV và JSON giữ giá trị nhị phân

Trước đây cột `bytea`, `varbinary` hay `blob` bị export thành ô trống (CSV) hoặc `""` (JSON). Giờ mỗi giá trị nhị phân được ghi thành `0x` và mã hex của nó, đủ mọi byte.

- [ ] Kết nối `Local PG`, chạy:
  ```sql
  create table thu_bin (id int primary key, data bytea);
  insert into thu_bin values (1, '\x00ff10'), (2, '\x'), (3, null);
  ```
  Chuột phải `thu_bin` trong cây, chọn **Export…**, chọn **CSV**.
  **Đúng:** dưới các lựa chọn có dòng "Binary values are written as hex text with a 0x prefix, as in 0x00ff." Chọn **SQL INSERT** thì dòng đó biến mất.
- [ ] Export CSV ra file, mở bằng Notepad.
  **Đúng:** dòng 1 là `1,0x00ff10`, dòng 2 là `2,0x`, dòng 3 là `3,` (NULL là ô trống).
- [ ] Export JSON ra file, mở bằng Notepad.
  **Đúng:** `"data": "0x00ff10"`, `"data": "0x"`, `"data": null`.
- [ ] Làm lại với SQL Server (`varbinary(10)`, giá trị `0x00FF10`) và SQLite (`blob`, giá trị `X'00ff10'`).
  **Đúng:** cùng kết quả, hex viết thường.
- [ ] Dọn: `drop table thu_bin;`

---

## A3. Lưu sửa, Import và Generate Rows không kết thúc transaction của người dùng

Trước đây, khi trong editor còn một `BEGIN` chưa `COMMIT`, việc lưu sửa trên tab bảng (hay Import, Generate Rows) cùng database sẽ `COMMIT` luôn phần việc dở của người dùng, hoặc `ROLLBACK` mất nó khi lưu lỗi. Giờ chúng chạy dưới một savepoint bên trong transaction đó: lỗi thì chỉ phần của chúng bị huỷ, transaction của người dùng vẫn mở.

### PostgreSQL

- [ ] Kết nối `Local PG`, chạy:
  ```sql
  create table thu_tx (id int primary key, a text);
  insert into thu_tx values (1, 'a');
  begin;
  insert into thu_tx values (2, 'b');
  ```
  Mở `thu_tx` trong cây, sửa ô `a` của dòng 1 thành `x`, nhấn **Ctrl+S** rồi **Save**.
  **Đúng:** lưu thành công. Ở tab query chạy `select * from thu_tx order by id;`: thấy `1 x` và `2 b`. Chạy `select now() = statement_timestamp() as khong_co_transaction_mo;`: ra `false` (transaction vẫn mở).
- [ ] Mở một kết nối thứ hai tới cùng database (hoặc psql), chạy `select * from thu_tx;`.
  **Đúng:** chỉ thấy `1 a`: chưa có gì được commit.
- [ ] Ở tab bảng, sửa dòng 1 sang `id = 2` (trùng khoá), lưu.
  **Đúng:** báo lỗi trùng khoá. Ở tab query, `select * from thu_tx order by id;` vẫn chạy được (không báo "current transaction is aborted") và vẫn thấy `1 x`, `2 b`.
- [ ] Chuột phải `thu_tx`, **Import…** một file CSV có `id,a` / `3,c`.
  **Đúng:** import xong; `select count(*) from thu_tx;` ra `3`; kết nối thứ hai vẫn chỉ thấy 1 dòng.
- [ ] Chạy `rollback;` rồi `select * from thu_tx;`.
  **Đúng:** chỉ còn `1 a`: rollback của người dùng huỷ cả phần sửa và phần import.
- [ ] Không `begin`, sửa một ô rồi lưu.
  **Đúng:** như trước: lưu và commit ngay, kết nối thứ hai thấy giá trị mới.
- [ ] Dọn: `drop table thu_tx;`

### SQL Server

- [ ] Kết nối SQL Server, trong một database thử, chạy:
  ```sql
  create table thu_tx (id int primary key, a nvarchar(10));
  insert into thu_tx values (1, N'a');
  SET XACT_ABORT ON;
  BEGIN TRANSACTION;
  insert into thu_tx values (2, N'b');
  ```
  Làm lại các bước của PostgreSQL ở trên (sửa, sửa trùng khoá, Import).
  **Đúng:** sau mỗi bước `select @@TRANCOUNT;` vẫn ra `1`; lỗi trùng khoá chỉ huỷ phần sửa đó; `select @@OPTIONS & 16384;` vẫn ra `16384` (XACT_ABORT vẫn bật như người dùng đặt).
- [ ] Chạy `ROLLBACK TRANSACTION;` rồi `select * from thu_tx;`.
  **Đúng:** chỉ còn `1 a`.
- [ ] Dọn: `drop table thu_tx;`

---

## A4. Sửa ô nhị phân ghi đúng các byte

Trước đây sửa một ô `bytea` từ `0x00ff` thành `0x00fe` thì PostgreSQL lưu sáu ký tự ASCII `0x00fe`, SQLite lưu chữ vào cột blob. Giờ giá trị hex được đọc thành byte.

- [ ] Kết nối `Local PG`, chạy `create table thu_bin2 (id int primary key, data bytea); insert into thu_bin2 values (1, '\x00ff');`. Mở `thu_bin2`, nhấn F2 trên ô `data`.
  **Đúng:** ô sửa bắt đầu bằng `0x00ff`.
- [ ] Sửa thành `0x00fe`, Enter, **Ctrl+S**.
  **Đúng:** preview ghi `SET "data" = '\x00fe'::bytea`. Save xong, chạy `select length(data), data from thu_bin2;`: ra `2` và `\x00fe` (hai byte, không phải sáu).
- [ ] Sửa ô thành `abc`, **Ctrl+S**.
  **Đúng:** không lưu, báo lỗi nói cột nhận byte viết dạng hex như `0x00ff`.
- [ ] Làm lại với SQL Server (`varbinary(10)`) và SQLite (`blob`).
  **Đúng:** preview ghi `0x00fe` (SQL Server) và `X'00fe'` (SQLite); `select datalength(data)` / `select length(data), typeof(data)` ra `2` (và `blob` ở SQLite). Ở SQLite, gõ `abc` thì lưu thành chữ `abc` (SQLite cho mọi kiểu trong mọi cột).
- [ ] Dọn: `drop table thu_bin2;`

---

## A5. SQL Server: ngày giờ không bị đọc lẫn ngày với tháng

Trước đây Kemable viết ngày giờ dạng `'2024-01-02 10:00:00'`; với login dùng ngôn ngữ British (hay French, German), cột `datetime` đọc nó thành ngày 1 tháng 2, không báo lỗi. Giờ Kemable viết `'2024-01-02T10:00:00'` (có chữ `T`) và ngày không giờ là `'20240102'`, mọi ngôn ngữ đều đọc như nhau.

- [ ] Kết nối SQL Server, chạy:
  ```sql
  SET LANGUAGE British;
  create table thu_ngay (id int primary key, d datetime, d2 datetime2, dt date);
  insert into thu_ngay values (1, '20240102 10:00', '20240102 10:00', '20240102');
  ```
  Mở `thu_ngay`, sửa ô `d2` thành `2024-03-04 05:06`, **Ctrl+S**.
  **Đúng:** preview ghi `[d2] = N'2024-03-04T05:06:00'` và `WHERE [id] = 1`. Save xong, `select month(d2), day(d2) from thu_ngay;` ra `3` và `4`.
- [ ] Chuột phải `thu_ngay`, **Export…**, **SQL INSERT**.
  **Đúng:** file có `N'2024-01-02T10:00:00'` cho `d` và `N'20240102'` cho `dt`. Chạy lại câu INSERT đó (sau `delete from thu_ngay;`) trong phiên đang `SET LANGUAGE British`: `select month(d), day(d) from thu_ngay;` ra `1` và `2`.
- [ ] Import một file CSV `id,d,d2,dt` / `2,2024-01-02 10:00,2024-01-02 10:00,2024-01-02` vào `thu_ngay` (trong phiên British).
  **Đúng:** dòng `id = 2` có `month(d) = 1`, `day(d) = 2`.
- [ ] Dọn: `drop table thu_ngay;`

---

## A6. SQLite: số gõ vào cột chữ giữ nguyên

Trước đây gõ `1.10` vào cột TEXT của SQLite thì lưu thành `1.1`, `10.00` thành `10.0`, một mã 20 chữ số thành `1.23456789012346e+19`. Giờ giá trị đi theo kiểu khai báo của cột.

- [ ] Mở một database SQLite (in-memory cũng được), chạy:
  ```sql
  create table thu_so (id integer primary key, ma text, n integer, r real, x);
  insert into thu_so (id) values (1);
  ```
  Mở `thu_so`, gõ `ma` = `1.10`, `n` = `42`, `r` = `2.5`, `x` = `12345678901234567890`, lưu.
  **Đúng:** preview ghi `"ma" = '1.10'`, `"n" = 42`, `"r" = 2.5`, `"x" = '12345678901234567890'`. Sau khi lưu, `select ma, typeof(ma), n, typeof(n), r, typeof(r), x, typeof(x) from thu_so;` ra `1.10 text`, `42 integer`, `2.5 real`, `12345678901234567890 text`. (Cột khai báo INTEGER thì SQLite tự đổi một số quá 64 bit thành số thực, Kemable không giữ được dạng chữ ở đó.)
- [ ] Sửa `n` thành `10.00`, lưu.
  **Đúng:** `n` là `10`, `typeof(n)` là `integer` (cột số đổi chữ thành số khi không mất gì).
- [ ] Import một CSV `id,ma,x` / `2,1.10,12345678901234567890` vào `thu_so`.
  **Đúng:** dòng `id = 2` có `ma = 1.10` và `x = 12345678901234567890`, cả hai là text.

---

## A7. Đóng cửa sổ khi còn việc dang dở

Đóng cửa sổ (nút X, Alt+F4, chuột phải trên taskbar › Close window) hỏi trước khi làm dừng một query đang chạy, một tab Live đang nghe, hoặc làm mất các sửa đổi bảng chưa lưu. Danh sách là danh sách bản cập nhật đã dùng (`docs/decisions/0028-updates.md`), trừ các trang `.sql` có thay đổi chưa lưu vào file: chúng được giữ lại như trước.

- [ ] Kết nối một SQLite, không làm gì thêm, nhấn **Alt+F4**.
  **Đúng:** cửa sổ đóng ngay, không hỏi gì.
- [ ] Mở lại, mở một bảng, sửa một ô (chưa nhấn Ctrl+S), bấm nút **X** của cửa sổ.
  **Đúng:** hộp thoại **Close Kemable?** hiện danh sách có dòng `<tên bảng> has table edits not saved: they are lost.`; cửa sổ vẫn mở.
- [ ] Nhấn **Esc** (hoặc **Back**).
  **Đúng:** hộp thoại đóng, sửa đổi vẫn còn (ô vẫn tô màu).
- [ ] Bấm **X** lần nữa, rồi **Close anyway** (hoặc Enter).
  **Đúng:** cửa sổ đóng. Mở lại: các trang query vẫn còn, vị trí cửa sổ đúng chỗ cũ, sửa đổi bảng không còn.
- [ ] Chạy một query dài (SQLite: `with recursive c(x) as (select 1 union all select x + 1 from c) select count(*) from c;`), trong lúc **Running**, chuột phải vào biểu tượng trên taskbar › **Close window**.
  **Đúng:** hộp thoại có dòng `A query is running: it stops.`. **Back** thì query vẫn chạy; Stop nó rồi đóng thì không hỏi nữa.
- [ ] Với PostgreSQL: mở tab Live (Ctrl+Alt+L), nghe `orders`, rồi **Alt+F4**.
  **Đúng:** hộp thoại có dòng `Live is listening: it stops.`.
- [ ] Mở một file `.sql`, gõ thêm vài chữ (chưa Ctrl+S), **Alt+F4**.
  **Đúng:** không hỏi; cửa sổ đóng. Mở lại: trang của file vẫn còn chữ vừa gõ và vẫn đánh dấu chưa lưu.

---

## A8. Một kết quả dừng ở khoảng 25% bộ nhớ máy

Fetch all và "No limit" không còn giới hạn số dòng, nên mỗi kết quả dừng khi giữ khoảng một phần tư bộ nhớ vật lý của máy, và nói "Stopped at N MB" như khi dừng ở giới hạn dòng.

- [ ] Mở SQLite `:memory:` (hoặc một file trống), chọn giới hạn **No limit** trên thanh trang, chạy:
  ```sql
  with recursive c(x) as (select 1 union all select x + 1 from c limit 200000000)
  select x, printf('%.200c', 'x') as dai from c;
  ```
  Mở Task Manager, xem bộ nhớ của Kemable.
  **Đúng:** bộ nhớ tăng tới khoảng một phần tư RAM của máy rồi dừng; thanh kết quả ghi **Stopped at N MB** (N gần một phần tư RAM, tính bằng MB), không có nút **Fetch all**; app vẫn cuộn, sắp xếp, lọc được; không bị tắt.
- [ ] Chạy lại câu trên với giới hạn **Limit 1,000**.
  **Đúng:** ghi **Stopped at 1,000 rows** và có nút **Fetch all** như trước.
- [ ] Firebase (nếu có project thử): ở console chạy `rtdb /` trên một Realtime Database lớn (hàng trăm MB).
  **Đúng:** báo lỗi "The answer is too big to hold in memory: read it with a limit"; `rtdb / limit 100` vẫn đọc được.

---

## A9. Stop giữ nguyên session SQL Server

**Chuẩn bị:** SQL Server ở `localhost`, đăng nhập bằng Windows Authentication; chạy app bằng `pnpm tauri dev` trong `ui/windows`. Phần tự động là các test live của driver SQL Server (`core/crates/drivers/mssql/src/tests.rs`, chạy với `KEMABLE_MSSQL_URL=sqlserver://localhost`).

Trước đây bấm **Stop** khi kết quả đã về gần hết làm driver bỏ session và mở session mới: bảng tạm, `SET`, `USE` và transaction đang mở mất hết mà không báo gì. Giờ Stop chỉ dừng câu lệnh; session vẫn là session cũ.

- [ ] Mở một tab query trên SQL Server, chạy từng câu (mỗi lần một câu, **Ctrl+Enter**):
  ```sql
  create table #kemable_a9 (a int)
  set dateformat dmy
  begin transaction
  select @@spid as spid, @@trancount as trancount, db_name() as db
  ```
  Ghi lại `spid` và `db`. **Đúng:** `trancount` là 1.
- [ ] Đặt con trỏ trên câu dưới, nhấn **Ctrl+Alt+Enter** (chạy không giới hạn), rồi bấm **■ Stop** khi status bar đang đếm dòng:
  ```sql
  select top 3000000 a.object_id from sys.all_objects a cross join sys.all_objects b
  ```
  **Đúng:** dừng ngay; Messages báo "Stopped"; không có thông báo mất kết nối.
- [ ] Chạy:
  ```sql
  select @@spid as spid, @@trancount as trancount, db_name() as db,
         case when object_id('tempdb..#kemable_a9') is null then 'mất' else 'còn' end as temp_table,
         (select date_format from sys.dm_exec_sessions where session_id = @@spid) as date_format
  ```
  **Đúng:** `spid` và `db` giống lần trước, `trancount` là 1, `temp_table` là `còn`, `date_format` là `dmy`.
- [ ] Chạy `waitfor delay '00:00:30'`, bấm **■ Stop** sau 1–2 giây, rồi chạy lại câu kiểm tra.
  **Đúng:** dừng ngay; cùng `spid`, `trancount` vẫn là 1.
- [ ] Dọn: `rollback`, `drop table #kemable_a9`, `set dateformat mdy`.

---

## B10. Kết nối lại sau khi máy ngủ, mất VPN hoặc server khởi động lại

Một phiên báo mất kết nối thì bị bỏ; việc đọc (cây, gợi ý, đếm dòng) được thử lại một lần trên phiên mới. Câu chạy, lưu sửa và Import không bao giờ bị gửi lại: lần sau mới mở phiên mới.

- [ ] Kết nối PostgreSQL, mở cây tới một bảng. Khởi động lại dịch vụ PostgreSQL (`services.msc` > postgresql > Restart).
  **Đúng:** bấm Ctrl+R làm mới cây: cây hiện lại bình thường, không báo "The connection was lost".
- [ ] Sau khi khởi động lại server lần nữa, chạy `select 1;` ở tab query.
  **Đúng:** lần chạy đầu báo **The connection was lost**; chạy lại lần hai thì ra `1`, không phải ngắt kết nối rồi kết nối lại.
- [ ] Làm như trên với SQL Server (Restart dịch vụ SQL Server).
  **Đúng:** giống PostgreSQL.
- [ ] Kết nối qua SSH tunnel. Ngắt mạng (tắt Wi-Fi hoặc ngắt VPN) khoảng 2 phút cho phiên SSH đứt, bật lại. Bấm Ctrl+R ở cây, rồi chạy `select 1;`.
  **Đúng:** cây và câu chạy hoạt động lại (lần chạy đầu có thể báo mất kết nối), không cần ngắt kết nối thủ công.
- [ ] RabbitMQ: mở console, chạy `publish amq.topic test x`. Khởi động lại RabbitMQ, chạy lại lệnh đó.
  **Đúng:** lệnh thứ hai chạy được (kết nối và channel được mở lại).

---

## B11. SQLite: gợi ý và cây không dừng câu đang chạy

- [ ] Mở SQLite `:memory:`, chạy một câu dài (khoảng 10 giây):
  ```sql
  with recursive c(x) as (select 1 union all select x + 1 from c limit 100000000) select count(*) from c;
  ```
  Trong lúc nó chạy, mở tab query khác, gõ `select * from ` để hiện gợi ý, và mở rộng cây ở sidebar.
  **Đúng:** gợi ý và cây có thể chưa ra (chờ câu kia), nhưng câu dài vẫn chạy tới hết và ra `100000000`; không báo "The server took too long".
- [ ] Chạy lại câu dài, bấm **Stop** (Esc) ngay sau khi bấm Run.
  **Đúng:** dừng trong khoảng một giây, báo **Stopped**.

---

## B12. Mở kết nối khác khi một câu đang chạy

Khi một câu đang chạy, mở kết nối khác bị từ chối và nhắc dừng câu trước, như Disconnect. Đóng kết nối thì câu của nó dừng theo. Sự kiện của một lần chạy cũ không bao giờ hiện trên trang của kết nối mới.

- [ ] Kết nối PostgreSQL, chạy `select pg_sleep(30);`. Trong lúc nó chạy, mở một kết nối đã lưu khác (Ctrl+K).
  **Đúng:** kết nối mới không mở; dải lỗi ghi "Stop the running query first (Esc)". Trang vẫn hiện câu đang chạy.
- [ ] Nhấn **Esc** để dừng, rồi mở lại kết nối kia.
  **Đúng:** mở được; trang mới không có kết quả, lỗi hay chữ "finished" nào của lần chạy cũ.
- [ ] SQL Server: chạy `waitfor delay '00:00:30';`, nhấn Esc, mở ngay kết nối khác.
  **Đúng:** kết nối mới mở trong vài giây, không đứng ở "Running".

---

## B13. Báo lỗi khi ứng dụng không mở được

Khi bước khởi động lỗi (thiếu WebView2, không tạo được thư mục dữ liệu), ứng dụng hiện một hộp thoại của Windows ghi lý do rồi mới thoát, thay vì tắt im lặng. Thư mục WebView của mỗi người dùng Windows là của riêng họ: `%LOCALAPPDATA%\<identifier>\webview`, không còn là `data\webview`.

- [ ] Đóng Kemable bản dev. Đổi tên thư mục `%APPDATA%\dev.kemable.app` thành `dev.kemable.app.bak`, rồi tạo một **file** rỗng tên `dev.kemable.app` ở đúng chỗ đó. Chạy `pnpm tauri dev`.
  **Đúng:** một hộp thoại lỗi của Windows tiêu đề **Kemable could not start**, có dòng lỗi về thư mục; bấm **OK** thì ứng dụng thoát. Xoá file rỗng, đổi tên thư mục `.bak` về như cũ.
- [ ] Mở Kemable bình thường, xem `%LOCALAPPDATA%\dev.kemable.app\webview` (bản cài: `%LOCALAPPDATA%\kemable.app\webview`).
  **Đúng:** thư mục có, chứa `EBWebView`; trong thư mục dữ liệu (`data\` cạnh exe, hoặc `%APPDATA%\dev.kemable.app`) không có thư mục `webview` mới được tạo.
- [ ] Bản cài cho mọi người (Everyone) trên máy có hai tài khoản Windows: tài khoản thứ nhất mở Kemable và để đó, chuyển sang tài khoản thứ hai (Switch user), mở Kemable.
  **Đúng:** ứng dụng mở được ở cả hai tài khoản, cùng danh sách kết nối đã lưu.

---

## B14. `connections.json` hỏng

File kết nối đã lưu không đọc được thì ứng dụng nói rõ, kèm đường dẫn file và nút mở thư mục, thay vì hiện như chưa lưu kết nối nào. Lưu kết nối vẫn từ chối ghi đè lên file đó.

- [ ] Đóng Kemable bản dev. Sao lưu `%APPDATA%\dev.kemable.app\connections.json`, rồi mở file bằng Notepad, xoá dấu `}` cuối cùng, lưu lại. Mở Kemable.
  **Đúng:** màn hình Welcome có dải đỏ **Saved connections: A file Kemable keeps cannot be read; it is left as it is**, dòng dưới ghi `cannot understand ...\connections.json: ...`; có nút **Open its folder**.
- [ ] Bấm **Open its folder**.
  **Đúng:** Explorer mở đúng thư mục chứa `connections.json`.
- [ ] Nhấn **Ctrl+K**.
  **Đúng:** nhóm **Saved connections** có một dòng **Saved connections cannot be read** (biểu tượng cảnh báo), không phải danh sách trống; chọn nó thì mở thư mục.
- [ ] Nhấn **Ctrl+N**, chọn PostgreSQL.
  **Đúng:** form hiện ngay lỗi đỏ cùng nội dung. Điền đủ, bấm **Connect** (có lưu): không lưu được, báo lỗi; file `connections.json` vẫn y như lúc bị sửa (không bị ghi đè).
- [ ] Đóng Kemable, chép file sao lưu về chỗ cũ, mở lại.
  **Đúng:** không còn dải đỏ; Ctrl+K có lại các kết nối đã lưu.

---

## B15. Mật khẩu đã lưu chỉ gửi tới nơi nó được lưu cho

Mật khẩu (hoặc token) giữ trong Credential Manager gắn với engine, host, port, user và SSH server của kết nối lúc lưu (`docs/decisions/0033-bound-passwords.md`). Khi `connections.json` bị đổi các thứ đó (ví dụ bởi một người dùng Windows khác trên cùng máy), Kemable không gửi mật khẩu đã lưu mà hỏi lại. `known_hosts` của Kemable nằm trong thư mục riêng của từng người dùng Windows.

- [ ] Lưu một kết nối PostgreSQL `Local PG` có **Save password**. Mở nó qua Ctrl+K.
  **Đúng:** kết nối ngay, không hỏi mật khẩu (mật khẩu lưu từ trước bản này được gắn với kết nối như đang lưu, một lần).
- [ ] Đóng Kemable. Mở `connections.json` (thư mục dữ liệu) bằng Notepad, đổi `"host"` của `Local PG` thành `127.0.0.1` (khác chữ `localhost` đã lưu), lưu lại. Mở Kemable, mở `Local PG`.
  **Đúng:** hộp thoại **Password for Local PG** ghi `This connection now points to postgres@127.0.0.1:5432, not where its password was kept for...`; chưa có kết nối nào tới server (xem `select * from pg_stat_activity` từ một client khác nếu muốn chắc).
- [ ] Gõ đúng mật khẩu, **Connect**. Ngắt kết nối, mở lại `Local PG`.
  **Đúng:** lần đầu kết nối được; lần sau không hỏi nữa (mật khẩu được giữ cho host mới).
- [ ] Đổi `"host"` trong file về `localhost`, mở lại.
  **Đúng:** hỏi lại một lần (mật khẩu giờ gắn với `127.0.0.1`); gõ xong thì lần sau không hỏi.
- [ ] Mở form sửa kết nối (menu kết nối › Edit), đổi Port thành `5433`, để trống ô Password, bấm **Save**. Đổi lại `5432`, **Save**, rồi mở kết nối.
  **Đúng:** không hỏi mật khẩu: sửa trong form của chính mình thì mật khẩu đi theo.
- [ ] Với một kết nối qua SSH tunnel có lưu mật khẩu SSH: sửa `"ssh"` › `"host"` trong `connections.json` sang một máy khác, mở kết nối.
  **Đúng:** dải đỏ **This connection's SSH server changed since its SSH password was kept: edit the connection to enter it again**, có nút **Edit connection** mở form; không có kết nối SSH nào được mở.
- [ ] Bản cài cho mọi người: kết nối qua SSH tới một server mới, bấm **Trust this key and connect**. Xem `%APPDATA%\kemable.app\known_hosts` và `data\known_hosts` cạnh exe.
  **Đúng:** khoá nằm trong `%APPDATA%\kemable.app\known_hosts`; `data\known_hosts` không được ghi.

---

## C16. Bộ cài mang theo văn bản giấy phép

Bộ cài đặt `LICENSE.txt` (văn bản AGPL-3.0) cạnh `Kemable.exe`; Settings › About › **License** mở nó bằng Notepad, như Third-party notices, không cần mạng hay GitHub.

- [ ] Build bộ cài (`pnpm tauri build`), cài, rồi xem thư mục cài.
  **Đúng:** có `LICENSE.txt` và `THIRD-PARTY-NOTICES.md` cạnh `Kemable.exe`; `LICENSE.txt` bắt đầu bằng `GNU AFFERO GENERAL PUBLIC LICENSE`.
- [ ] Rút mạng, mở **Settings** (Ctrl+,) › About › **License**.
  **Đúng:** Notepad mở `LICENSE.txt` trong thư mục cài; không mở trình duyệt.
- [ ] Bản dev (`pnpm tauri dev`): bấm **License**.
  **Đúng:** Notepad mở `LICENSE.txt` từ thư mục build (`target\debug`).

---

## C17. Website của Kemable (`site/`) và đường dẫn Terms, Privacy trong app

`docs/decisions/0030-website.md`. Cần Node 22+ và pnpm.

### C17.1 Chạy trên máy

- [ ] Trong `site/`: `pnpm install`, rồi `pnpm build`.
  **Đúng:** build xong trong vài giây, không cần mạng (ngoài lúc cài); có `site/dist/index.html`, `terms.html`, `privacy.html`.
- [ ] `pnpm preview`, mở `http://localhost:4321`.
  **Đúng:** trang chủ nền tối, giống bản thiết kế: thanh trên có icon Kemable, Features, Engines, GitHub, nút Download; dòng "Fast is the feature."; video demo tự chạy, lặp, không tiếng; dòng engine; ba con số 2 ms, 1.07 s, 0.13 s; ba phần Edit, Ctrl K, Light or dark với ảnh; bốn thẻ nhỏ; khối "Open your data in seconds."; chân trang có Terms, Privacy, GitHub và nút đổi giao diện.
- [ ] Tab trình duyệt.
  **Đúng:** favicon là icon Kemable (ô vuông cam giữa hai góc trắng).
- [ ] Bấm **Features**, **Engines** trên thanh trên.
  **Đúng:** cuộn tới phần tương ứng của trang chủ.
- [ ] Bấm **Download**, **Download for Windows**, chip **New**.
  **Đúng:** mở `https://github.com/SoleBuilt/Kemable/releases/latest`.
- [ ] Bấm **SoleBuilt** ở chân trang.
  **Đúng:** mở `https://solebuilt.ducht.info.vn/`.
- [ ] Cuộn xuống chân trang, bấm nút mặt trời.
  **Đúng:** cả trang chuyển sang nền trắng ngay; nút thành mặt trăng. Tải lại trang: vẫn sáng, không chớp tối lúc tải. Bấm lại: về tối.
- [ ] Thu cửa sổ trình duyệt hẹp như điện thoại (hoặc F12, chế độ thiết bị, 375 px).
  **Đúng:** không có thanh cuộn ngang; thanh trên chỉ còn icon, Kemable và Download; ảnh nằm dưới chữ.
- [ ] Mở `/terms` và `/privacy` (từ chân trang).
  **Đúng:** đầu mỗi trang có khung "Draft for legal review."; nội dung nói đúng app làm gì: không telemetry, một lần hỏi `latest.json` của GitHub Releases lúc mở app, dữ liệu trong thư mục data trên máy, mật khẩu trong Windows Credential Manager, kết nối chỉ tới nơi người dùng chỉ định, AGPL và license thương mại.
- [ ] F12, tab Network, tải lại trang chủ.
  **Đúng:** mọi yêu cầu đều tới `localhost` (font cũng vậy), không có Google Fonts hay analytics; không file `.js` nào tải trước khi cuộn tới chân trang.

### C17.2 Triển khai lên Cloudflare (Worker)

`docs/decisions/0034-site-worker-and-measuring.md`. Làm theo `site/README.md` (Deploying) một lần: thêm hai secret. Database `kemable-site` đã có sẵn.

- [ ] Trước khi có secret: merge một thay đổi trong `site/` vào `main` (hoặc Actions › Site › Run workflow).
  **Đúng:** job **site** xanh (có bước **Test the Worker**); có thông báo "Built, not deployed. Add the CLOUDFLARE_API_TOKEN and CLOUDFLARE_ACCOUNT_ID secrets".
- [ ] Sau khi thêm `CLOUDFLARE_API_TOKEN` và `CLOUDFLARE_ACCOUNT_ID`: chạy lại workflow trên `main`.
  **Đúng:** bước **Apply the database's migrations** báo không có gì mới (hoặc áp file mới); bước **Deploy the Worker** in `kemable.ducht.info.vn (custom domain)`; vài phút sau `https://kemable.ducht.info.vn` hiện trang chủ. Cloudflare › Workers & Pages có Worker `kemable`, không có địa chỉ `workers.dev`.
- [ ] Mở `https://kemable.ducht.info.vn/terms` và `https://kemable.ducht.info.vn/privacy`.
  **Đúng:** mở thẳng trang, không chuyển hướng sang `/terms/`.
- [ ] F12 › Elements trên trang chủ, tìm `canonical`.
  **Đúng:** `<link rel="canonical" href="https://kemable.ducht.info.vn/">`; `/sitemap.xml` và `/llms.txt` cũng dùng domain này.
- [ ] Một pull request chỉ sửa `site/`.
  **Đúng:** job **site** chạy check, test và build, không deploy.

### C17.2b Danh sách chờ macOS và Linux

`docs/decisions/0030-website.md` (The waitlist), `site/README.md`.

- [ ] Trong `site/`, hai terminal: `pnpm worker`, rồi `pnpm dev`; mở `http://localhost:4321`.
  **Đúng:** dưới hai nút Download có dòng "macOS and Linux later" gạch chân; bấm vào thì cuộn tới khối waitlist: hàng 1 ô **Name**, hàng 2 ô **Email**, hàng 3 hai nút chọn **macOS** và **Linux** (có ô tick), hàng 4 nút **Notify me**.
- [ ] Bấm **Notify me** khi ô còn trống, khi email sai (`abc`), rồi khi đã điền đủ nhưng chưa chọn hệ điều hành nào.
  **Đúng:** trình duyệt chặn lại và chỉ ra chỗ cần sửa ("Pick macOS, Linux or both." cho lần cuối); không rời trang.
- [ ] Bấm **macOS**, rồi **Linux**, rồi bấm lại **macOS**. Dùng Tab tới một nút và bấm Space.
  **Đúng:** mỗi lần bấm đổi trạng thái tick, viền sáng lên khi được chọn; Tab có viền cam quanh nút đang được chọn, Space tick hoặc bỏ tick.
- [ ] Mở `http://localhost:4321/?utm_source=chatgpt.com&utm_campaign=test`, nhập tên, email đúng, chọn cả **macOS** và **Linux**, bấm **Notify me**.
  **Đúng:** chuyển sang trang "You are on the list." (vẫn ở `localhost:4321`); `npx wrangler@4.156.0 d1 execute kemable-site --local --command "SELECT * FROM waitlist"` có hai dòng `macos` và `linux` cho email đó, email viết thường, `source` là `chatgpt.com`, `medium` là `ai`, `campaign` là `test`, `signal` là `utm`.
- [ ] Gửi lại cùng email, chỉ chọn **Linux**, với tên khác.
  **Đúng:** vẫn hai dòng cho email đó; dòng `linux` mang tên mới, giữ `joined_at` cũ.
- [ ] Tắt `pnpm worker`, gửi form từ `pnpm dev`.
  **Đúng:** lỗi (không có Worker để nhận); bật lại `pnpm worker` thì gửi được.
- [ ] Thu cửa sổ còn 375 px.
  **Đúng:** bốn hàng xếp dọc, hai nút hệ điều hành vẫn cạnh nhau, không có thanh cuộn ngang.
### C17.2c Đếm nguồn truy cập và cookie

`docs/decisions/0034-site-worker-and-measuring.md`. Trên máy như C17.2b (`pnpm worker` và `pnpm dev`, mở `http://localhost:4321`), mỗi bước một cửa sổ InPrivate mới (chưa có cookie).

- [ ] Mở `http://localhost:4321/?utm_source=facebook&utm_campaign=launch`.
  **Đúng:** thông báo cookie hiện ở đáy trang, hai nút **No thanks** và **Allow** to bằng nhau, có link Privacy; F12 › Application › Cookies: trống; Network: không có gì tới `googletagmanager.com` hay `facebook.net`; có một `POST /api/visit` trả 204.
- [ ] Bấm **Features**, rồi mở `/privacy` từ chân trang, rồi tải lại trang (F5).
  **Đúng:** không có thêm `POST /api/visit` (đi trong site và tải lại không tính là lượt vào mới).
- [ ] Bấm **Download for Windows**.
  **Đúng:** có một `POST /api/visit` nữa; bảng `visits` có dòng `download` với `source` là `facebook`, `signal` là `utm`.
- [ ] Bấm **No thanks**.
  **Đúng:** thông báo biến mất; cookie chỉ còn `kemable_consent=none`; mở trang khác không hỏi lại.
- [ ] Cửa sổ mới, mở trang chủ, bấm **Allow**.
  **Đúng:** có cookie `kemable_consent=all`; khi chưa điền ID trong `site/src/analytics.ts`, vẫn không tải gì của Google hay Meta, và thông báo chỉ nói về việc nhớ link đầu tiên.
- [ ] Chân trang › **Cookies** › **No thanks**.
  **Đúng:** trang tải lại; cookie `kemable_src` và `_ga`, `_fbp` (nếu có) bị xóa, chỉ còn `kemable_consent=none`.
- [ ] Đã điền ID Google Analytics và Meta Pixel (sau khi tạo, `site/README.md`), deploy, mở trang chủ, bấm **Allow**.
  **Đúng:** thông báo nêu tên Google Analytics và Meta Pixel; sau Allow mới có yêu cầu tới `googletagmanager.com` và `connect.facebook.net`; GA › Realtime thấy lượt xem; Meta › Events Manager › Test events thấy `PageView`; đăng ký waitlist thì thấy `generate_lead` / `Lead`.
- [ ] Mở `/privacy`.
  **Đúng:** có các mục đếm nguồn không cookie, cookie và Google Analytics, Meta Pixel (chỉ khi đồng ý), waitlist có lưu nguồn; ngày "Last updated 18 November 2026".
- [ ] Mở `/security` (chân trang › **Security**), rồi `/terms`.
  **Đúng:** Security nói: mật khẩu trong Windows Credential Manager và chỉ gửi tới đúng host, port, user đã lưu; kết nối đi thẳng tới server, TLS, **Verify full**; SSH host key mới thì hỏi, bị đổi thì từ chối; không telemetry; bản cập nhật được ký; báo lỗ hổng riêng qua GitHub hoặc email, dẫn tới `SECURITY.md`. Privacy có bảng cookie, bên nhận dữ liệu, thời gian lưu, quyền của bạn. Terms có thêm luật áp dụng (Việt Nam). Trên 375 px các bảng cuộn ngang trong khung, trang không cuộn ngang.
- [ ] Sau khi deploy: chạy các câu trong `site/README.md`, mục "Where visitors come from".
  **Đúng:** số lượt vào và tải về theo nguồn; ChatGPT nằm ở `medium = 'ai'`.

### C17.3 Trong app

- [ ] `pnpm tauri dev`, mở app lần đầu với thư mục data trống.
  **Đúng:** không có hộp thoại Terms (vẫn ẩn, như trước).
- [ ] Tìm trong mã nguồn và ADR tên sản phẩm cũ dùng làm trang tạm: `git grep -i -n "clause/terms"`.
  **Đúng:** không còn kết quả; `open_terms` trong `commands.rs` chỉ biết `https://kemable.ducht.info.vn/terms` và `/privacy`.

---

## C18. README tiếng Anh, bản tiếng Việt bên cạnh

- [ ] Mở trang repository trên GitHub.
  **Đúng:** README bằng tiếng Anh: Kemable là gì, ảnh chụp app (giao diện sáng) hiện ra, phần Download dẫn tới `releases/latest`, bảng engine, Building from source, License.
- [ ] Bấm **Tiếng Việt** ở đầu README.
  **Đúng:** mở `README.vi.md`; ở đầu có **English** dẫn ngược về `README.md`.
- [ ] Làm theo phần "Chạy từ mã nguồn" của `README.vi.md` trên một máy sạch.
  **Đúng:** màn hình Welcome có đúng các nút README nhắc (**Open an in-memory database**, **Open file…**, **New connection…**); câu SQL 2 triệu dòng chạy được.

---

## C19. Ký Authenticode cho installer

Mục này còn mở cho tới khi có chứng chỉ code signing (`docs/decisions/0028-updates.md`, Authenticode).

- [ ] Khi **chưa** có secret `WINDOWS_CERTIFICATE`: đẩy một tag phiên bản (hoặc xem lần release gần nhất).
  **Đúng:** hai bước **Import the code-signing certificate** và **Check the Authenticode signature** bị bỏ qua (skipped); bản nháp release vẫn có installer, `.sig` và `latest.json` như trước.
- [ ] Sau khi thêm `WINDOWS_CERTIFICATE` và `WINDOWS_CERTIFICATE_PASSWORD` theo ADR 0028: đẩy tag phiên bản kế tiếp.
  **Đúng:** bước **Check the Authenticode signature** in "signed by" kèm tên chứng chỉ; workflow xanh.
- [ ] Tải installer từ bản nháp, chuột phải › Properties › **Digital Signatures**.
  **Đúng:** có chữ ký của SoleBuilt, có timestamp. Chạy installer: hộp thoại UAC hoặc SmartScreen ghi tên nhà phát hành, không còn "Unknown publisher".
- [ ] Từ bản đã cài trước đó, cập nhật lên bản đã ký (Settings › Updates › Check now › Update).
  **Đúng:** cập nhật thành công, không dừng ở lỗi chữ ký (`update.signature`).

---

## D1. Xoá trắng một ô không phải chữ thì thành NULL

Trước đây xoá hết chữ trong một ô số hay ngày của SQL Server rồi lưu thì gửi `N''`, server lưu thành `0` hoặc `1900-01-01`. Giờ ô không phải chữ được lưu `NULL`; ô chữ vẫn giữ chuỗi rỗng.

- [ ] Kết nối SQL Server, chạy `create table thu_rong (id int primary key, n int, d datetime, t nvarchar(10)); insert into thu_rong values (1, 5, '20240102', N'abc');`. Mở `thu_rong`.
- [ ] Nhấn F2 trên ô `n`, xoá hết chữ, Enter; làm tương tự với `d` và `t`. **Ctrl+S**.
  **Đúng:** preview ghi `[n] = NULL`, `[d] = NULL`, `[t] = N''`. Sau khi lưu, `select n, d, t, len(t) from thu_rong;` ra `NULL`, `NULL`, chuỗi rỗng và `0`.
- [ ] Làm lại với PostgreSQL (`int`, `timestamp`, `text`).
  **Đúng:** preview ghi `"n" = NULL`, `"d" = NULL`, `"t" = ''`; trước đây PostgreSQL báo lỗi kiểu.
- [ ] Dọn: `drop table thu_rong;`

---

## D2. Sửa ô nhiều dòng giữ nguyên kiểu xuống dòng

Trước đây ô có xuống dòng kiểu Windows (CRLF) sau khi sửa bị đổi hết thành LF. Giờ giá trị đang có CRLF thì sau khi sửa vẫn là CRLF.

- [ ] Mở SQLite in-memory, chạy `create table thu_dong (id integer primary key, note text); insert into thu_dong values (1, 'a' || char(13, 10) || 'b'), (2, 'c' || char(10) || 'd');`. Mở `thu_dong`.
- [ ] Nhấn F2 trên `note` dòng 1, Escape mà không đổi gì; rồi F2 lại, Enter mà không đổi gì.
  **Đúng:** ô không bị đánh dấu là đã sửa (không có màu sửa, không có gì để lưu).
- [ ] F2 trên `note` dòng 1, đặt con trỏ cuối, Shift+Enter, gõ `c`, Enter; làm tương tự với dòng 2 (gõ `e`). Lưu.
  **Đúng:** `select id, hex(note) from thu_dong;` ra `610D0A620D0A63` cho dòng 1 (mọi dòng đều CRLF) và `630A640A65` cho dòng 2 (vẫn LF).

---

## D3. CSV phân biệt NULL với chuỗi rỗng

Trước đây Export CSV ghi NULL và chuỗi rỗng đều là ô trống, và Import (với "An empty field is NULL" bật sẵn) đọc mọi ô trống thành NULL. Giờ Export ghi NULL là ô trống không nháy, chuỗi rỗng là `""`; Import đọc ô trống không nháy là NULL (khi ô tick bật) và `""` là chuỗi rỗng.

- [ ] Mở SQLite in-memory, chạy `create table thu_csv (id integer primary key, t text); insert into thu_csv values (1, null), (2, ''), (3, 'x');`. Export `thu_csv` ra CSV, mở bằng Notepad.
  **Đúng:** `1,`, `2,""`, `3,x`.
- [ ] Chạy `delete from thu_csv;`, rồi Import chính file đó vào `thu_csv`.
  **Đúng:** hộp thoại Import ghi "An empty field is NULL ("" stays empty text)". Sau khi import, `select id, t is null, length(t) from thu_csv;` ra `1 1 NULL`, `2 0 0`, `3 0 1`.
- [ ] Làm lại bước Import với ô tick đó tắt.
  **Đúng:** dòng 1 cũng là chuỗi rỗng (`t is null` là `0`).

---

## D4. Không mất chữ chưa lưu trong các trang query

File trang (`pages\<hash>.json`) hoặc `ui.json` không đọc được thì được đổi tên để giữ lại (`<tên>.unreadable-<số>`), không bị ghi đè, và ứng dụng báo. Giới hạn 40 kết nối giữ trang bỏ trước những kết nối cũ nhất **không** có chữ chưa lưu; trang có chữ chưa lưu không bao giờ bị xoá.

- [ ] Đóng Kemable bản dev. Trong `%APPDATA%\dev.kemable.app\pages`, mở file `.json` mới nhất bằng Notepad, xoá vài ký tự cuối, lưu. Mở Kemable, mở lại kết nối có các trang đó.
  **Đúng:** dải vàng **Kemable could not read what it kept, so it set the file aside and did not write over it**, nội dung ghi `cannot understand ...json: ...; kept as ...json.unreadable-<số>`; có nút **Open its folder**. Trong thư mục `pages` có file `.unreadable-…` giữ nguyên chữ cũ.
- [ ] Gõ vài chữ vào trang query, đợi 1 giây, xem lại thư mục.
  **Đúng:** một file `.json` mới được ghi; file `.unreadable-…` vẫn còn, không đổi.
- [ ] Đóng Kemable, làm hỏng `ui.json` tương tự, mở lại.
  **Đúng:** cùng dải vàng, ngay khi mở, về `ui.json`; giao diện về cài đặt mặc định; `ui.json.unreadable-…` nằm cạnh.
- [ ] (Giới hạn 40) Trong thư mục `pages`, máy tự kiểm: `cargo test -p kemable-windows past_the_limit_pages_with_unsaved_text_are_never_dropped`.
  **Đúng:** test qua.

---

## D5. Mỗi trang một database

Database chọn ở thanh công cụ thuộc về trang đang xem, không còn là của cả cửa sổ.

- [ ] Kết nối PostgreSQL có ít nhất hai database (ví dụ `postgres` và `demo`). Ở **Query 1**, chọn `demo` trong ô database trên thanh công cụ.
  **Đúng:** ô ghi `demo`; status bar ghi `in demo`.
- [ ] Nhấn **Ctrl+T** mở **Query 2**.
  **Đúng:** Query 2 bắt đầu ở `demo` (database của trang vừa mở nó). Chọn `postgres` ở Query 2.
- [ ] Quay lại Query 1.
  **Đúng:** ô database ghi lại `demo`; chạy `select current_database();` ra `demo`. Sang Query 2 chạy lại câu đó ra `postgres`.
- [ ] Ở Query 1 (đang `demo`), mở một bảng của database `postgres` từ cây.
  **Đúng:** trang bảng hiện `postgres` trong ô database; quay về Query 1 vẫn là `demo`.
- [ ] Ở một trang chạy `\c postgres` (PostgreSQL) rồi sang trang khác.
  **Đúng:** chỉ trang đã chạy đổi database; trang kia giữ database của nó.

---

## D6. Run trong History không chạy câu của kết nối hay database khác

Run trong History chỉ chạy ngay khi câu đã chạy trên đúng kết nối này (so bằng khoá của kết nối: id của kết nối đã lưu, đường dẫn file SQLite, hoặc engine và tên) và đúng database trang đang dùng. Ngược lại, câu được đặt vào editor để đọc trước, nhấn F5 nếu muốn chạy. Run không còn đổi database của trang.

- [ ] Kết nối `Local PG` (đã lưu), chạy `select 1 as mot;`. Trong History bấm **Run** ở dòng đó.
  **Đúng:** câu được chèn vào editor và chạy ngay, ra `mot = 1`.
- [ ] Đổi database của trang sang database khác, bấm **Run** ở dòng cũ (đã chạy ở database trước).
  **Đúng:** câu chỉ được chèn vào editor, không chạy; status bar ghi `Put in the editor, not run: it ran in <database cũ>`; ô database không đổi.
- [ ] Lưu một kết nối thứ hai **cùng tên hiển thị** `Local PG` nhưng tới server khác; mở nó, bỏ tick **This connection** trong History, bấm **Run** ở một câu của kết nối đầu.
  **Đúng:** chỉ chèn vào editor, không chạy; status bar ghi `Put in the editor, not run: it ran on Local PG`.
- [ ] Các câu trong History từ trước bản này (chưa có khoá kết nối), bấm **Run**.
  **Đúng:** chỉ chèn vào editor, không chạy.

---

## D7. Hộp thoại Drop và Truncate ghi rõ database và kết nối

- [ ] Kết nối SQL Server `Local SQL` (hoặc PostgreSQL), trên cây chuột phải một bảng trong database `Sales` › **Drop…**.
  **Đúng:** nội dung ghi `The table dbo.<bảng> in the database Sales on Local SQL and everything in it go. This cannot be undone.`.
- [ ] Đổi database của trang sang database khác, rồi chuột phải cùng bảng trong `Sales` › **Truncate…**.
  **Đúng:** ghi `Every row of dbo.<bảng> in the database Sales on Local SQL is deleted.`: database của nút cây, không phải của trang.
- [ ] Với SQLite: chuột phải một bảng › **Drop…**.
  **Đúng:** ghi `in <tên file>` (không có chữ "database"), vì SQLite không có database để chọn.

---

## D8. Mở phiên của một database không làm đứng cả kết nối

Phiên của một database mở trong một tác vụ riêng: các database khác vẫn dùng được trong lúc chờ, và Stop hoặc hạn giờ của lời gọi kết thúc việc chờ.

- [ ] Kết nối PostgreSQL qua một mạng chậm, hoặc một Neon project đang ngủ (mất vài giây để thức). Trong cây, mở một database chưa mở lần nào; ngay lúc đó mở rộng database home và chạy `select 1;` ở tab query trên home.
  **Đúng:** cây của home và câu `select 1` ra ngay, không phải chờ database kia mở xong.
- [ ] Chọn database đang mở chậm ở thanh công cụ, chạy `select 1;`, nhấn **Esc** khi còn đang "Running".
  **Đúng:** dừng ngay, báo **Stopped**; chạy lại sau vài giây thì chạy được trên phiên đã mở xong.

---

## D9. Đường truyền chết giữa câu không để "Running" mãi

SQL Server và Redis bật TCP keepalive; D1 và Firebase hết chờ nếu server im quá 120 giây; câu SQL Server chạy lâu hơn 30 giây không còn bị cắt.

- [ ] SQL Server: chạy `waitfor delay '00:00:40'; select 1;`.
  **Đúng:** sau 40 giây ra `1`; không báo "The server took too long" ở giây thứ 30.
- [ ] SQL Server qua mạng (không phải localhost): chạy `waitfor delay '00:05:00';`, rồi rút dây mạng hoặc tắt Wi-Fi.
  **Đúng:** trong khoảng 2 phút báo **The connection was lost** (không treo "Running" mãi); cắm mạng lại, chạy `select 1;` thì chạy được.
- [ ] SQL Server: chạy `waitfor delay '00:00:20';` ở một tab; trong lúc đó mở tab khác và mở cây một bảng.
  **Đúng:** cây báo hết thời gian chờ hoặc chờ xong, không làm treo app; câu ở tab đầu vẫn chạy tới hết.
- [ ] Redis: mở console, ngắt mạng tới server khoảng 2 phút, chạy `PING`.
  **Đúng:** lần đầu báo mất kết nối hoặc lỗi; nối mạng lại, `PING` lần sau ra `PONG`.

---

## D10. PostgreSQL: Stop không bị chặn sau một lần dừng không trọn

Khi một lần chạy bị dừng mà lệnh huỷ không tới server, lần chạy sau phải dọn transaction cũ trước. Việc dọn đó giờ nằm trong vùng mà Stop theo dõi.

- [ ] Kết nối PostgreSQL, chạy `select pg_sleep(60);` rồi nhấn Esc ngay.
  **Đúng:** báo **Stopped** trong khoảng một giây.
- [ ] Chạy `select * from generate_series(1, 5000000);` (giới hạn 1,000 dòng), nhấn Esc khi còn "Running"; ngay sau đó chạy `select pg_sleep(30);` và nhấn Esc.
  **Đúng:** lần thứ hai cũng dừng trong khoảng một giây, không phải chờ.

---

## D11. RabbitMQ: publish chờ broker xác nhận

- [ ] Mở console RabbitMQ, chạy `publish khong_co_exchange x hello`.
  **Đúng:** báo **The query failed** kèm "NOT_FOUND - no exchange 'khong_co_exchange'", không phải "Done".
- [ ] Ngay sau đó chạy `publish amq.topic test.x hello`.
  **Đúng:** chạy được (channel bị broker đóng đã được mở lại).
- [ ] Chạy `publish amq.direct khong_ai_nghe hello` (không queue nào bind khoá này).
  **Đúng:** Done, kèm một dòng ở Messages: "The broker returned the message: no queue is bound to take it."

---

## D12. Phiên rảnh 10 phút được đóng

Phiên của một database không phải home, không dùng trong 10 phút, được đóng; lần dùng sau mở lại. Không đóng phiên home, phiên đang chạy câu, database có tab Live đang nghe, database có tab bảng còn sửa chưa lưu, hay phiên còn transaction mở.

- [ ] Kết nối PostgreSQL. Mở cây hai database khác home (ví dụ `db1`, `db2`). Ở một tab query chạy trên home:
  ```sql
  select datname, count(*) from pg_stat_activity where application_name = 'Kemable' group by datname;
  ```
  **Đúng:** `db1`, `db2` mỗi cái có 2 kết nối.
- [ ] Không dùng `db1`, `db2` trong 11 phút, chạy lại câu trên.
  **Đúng:** `db1`, `db2` không còn trong kết quả; home vẫn còn. Mở lại cây `db1` thì vẫn đọc được bình thường.
- [ ] Chọn `db1`, chạy `begin; create temp table t (x int);` (không commit). Để rảnh 11 phút, chạy `select * from t;` trên `db1`.
  **Đúng:** câu chạy được (phiên còn transaction mở nên không bị đóng). Chạy `rollback;` để dọn.
- [ ] Mở tab bảng của một bảng trong `db2`, sửa một ô nhưng không lưu. Để rảnh 11 phút, rồi Save.
  **Đúng:** phiên `db2` vẫn còn (câu kiểm tra ở bước 1 vẫn thấy `db2`), lưu được.

---

## D13. Chỉ một Kemable

Mở Kemable lần hai (cùng người dùng Windows, cùng bản) không mở cửa sổ thứ hai mà đưa cửa sổ đang chạy lên trước.

- [ ] Mở Kemable bản cài. Mở lại từ Start menu (hoặc nhấp đúp `Kemable.exe`).
  **Đúng:** không có cửa sổ mới; cửa sổ đang mở được đưa lên trước và nhận focus. Task Manager chỉ có một `Kemable.exe` (cùng các tiến trình WebView2 của nó).
- [ ] Thu nhỏ cửa sổ, mở Kemable lần nữa.
  **Đúng:** cửa sổ hiện lại từ thanh taskbar, đúng kích thước cũ.
- [ ] Trong lúc bản cài đang mở, chạy `pnpm tauri dev`.
  **Đúng:** bản dev vẫn mở được (identifier khác), hai cửa sổ cùng tồn tại.

---

## D14. Quyền của cửa sổ chỉ gồm những gì giao diện dùng

`capabilities/default.json` không còn `core:default`; chỉ liệt kê từng quyền giao diện gọi. Một quyền thiếu sẽ làm một nút hỏng âm thầm, nên thử từng thứ:

- [ ] Bản dev: kéo cửa sổ bằng thanh tiêu đề; nhấp đúp thanh tiêu đề; các nút thu nhỏ, phóng to/thu lại, đóng.
  **Đúng:** đều chạy; nhấp đúp phóng to rồi thu lại.
- [ ] Đổi kích thước cửa sổ, phóng to rồi thu lại.
  **Đúng:** nút phóng to đổi biểu tượng đúng trạng thái (cần `is-maximized` và sự kiện resize).
- [ ] **Ctrl+=**, **Ctrl+-**, **Ctrl+0**.
  **Đúng:** zoom đổi, status bar ghi mức zoom.
- [ ] Settings › About.
  **Đúng:** dòng đầu ghi `Kemable <phiên bản>` (cần `app:allow-version`).
- [ ] **Ctrl+O** mở file, Export một bảng ra CSV (hộp Save).
  **Đúng:** cả hai hộp thoại file mở.
- [ ] Chạy một query, mở tab Live (PostgreSQL), Export: kết quả, sự kiện và tiến độ đến bình thường (Channel không cần quyền).
- [ ] Đóng cửa sổ có sửa đổi bảng chưa lưu (A7): hộp hỏi hiện, **Close anyway** đóng được.
- [ ] Mở DevTools (F12) › Console trong suốt các bước trên.
  **Đúng:** không có lỗi `not allowed by ACL`.

---

## D15. Lỗi khi mở thư mục, About và Credential Manager được báo

Các nút **Open** (thư mục dữ liệu), **License**, **Source code › Open**, **Third-party notices › Open** trong Settings báo lỗi khi không mở được, thay vì im lặng. Lỗi của Windows Credential Manager có mã riêng (`secret.vault`), không còn bị ghi là lỗi file hay mạng.

- [ ] Bản dev: đổi tên `target\debug\THIRD-PARTY-NOTICES.md` thành tên khác, mở Settings › About › Third-party notices › **Open**.
  **Đúng:** dải đỏ dưới phần About: **Not opened: A file or network error stopped the call**, dòng dưới ghi đường dẫn file `... is not there`. Đổi tên lại như cũ.
- [ ] Bấm dấu X trên dải đỏ.
  **Đúng:** dải đóng. Bấm lại một nút mở thành công: không có dải.
- [ ] (Khó tạo trên máy thường) Khi Credential Manager từ chối (ví dụ chính sách nhóm chặn), lưu một kết nối có Save password.
  **Đúng:** lỗi ghi **Windows Credential Manager could not keep or read the password**, chi tiết bắt đầu `Windows Credential Manager: `.

---

## D16. Ghi chú phát hành 0.1.0

- [ ] Đọc `docs/releases/0.1.0.md` (cũng là nội dung bản nháp release trên GitHub).
  **Đúng:** không còn câu "Kemable now updates itself"; dòng cuối nói app tự hỏi GitHub một lần mỗi lần mở và cài khi người dùng chọn Update, không ngụ ý có bản trước đó.

---

## D17. SQLite: câu không sửa dòng thì không báo số dòng

Trước đây sau một `INSERT` 2.000 dòng, câu `CREATE TABLE` hay `CREATE VIEW` chạy tiếp theo cũng báo "2,000 rows affected", vì SQLite giữ số dòng của câu sửa dữ liệu trước đó. Giờ chỉ `INSERT`, `UPDATE`, `DELETE`, `REPLACE` báo số dòng.

- [ ] Mở SQLite in-memory, chạy cả khối (Ctrl+Enter trên từng câu, hoặc chạy hết):
  ```sql
  create table thu_dem (x int);
  with recursive c(x) as (select 1 union all select x + 1 from c limit 2000) insert into thu_dem select x from c;
  create view thu_dem_v as select * from thu_dem;
  update thu_dem set x = x where 0;
  drop view thu_dem_v;
  ```
  **Đúng:** câu `insert` báo 2,000 dòng; `create view` và `drop view` báo xong mà không có số dòng; `update ... where 0` báo 0 dòng.

---

## D18. Cột số dòng đủ rộng

Cột số dòng rộng theo số dòng của kết quả: đủ cho số dòng cuối, đo bằng chữ số rộng nhất của font Geist, đo lại khi font tải xong, và chừa chỗ cho dấu `− ` của dòng đánh dấu xoá.

- [ ] SQLite in-memory, chạy `with recursive c(x) as (select 1 union all select x + 1 from c limit 2000000) select x, x * 2, 'row ' || x from c` với **No limit**. Nhấn **Ctrl+End**.
  **Đúng:** số `2000000` nằm gọn trong cột số dòng, cách vạch cột đầu một khoảng như ở dòng 1; không chạm hay đè lên cột đầu.
- [ ] Cuộn bằng chuột, thanh cuộn và bàn phím trong lúc dòng còn đang về.
  **Đúng:** cột số dòng nở ra khi qua 9.999, 99.999, 999.999 dòng; cuộn vẫn mượt.
- [ ] Mở một bảng có trên 100.000 dòng (Fetch all), chọn vài dòng cuối, nhấn **Del**.
  **Đúng:** dòng ghi `− <số>`, cả dấu và số nằm trong cột, không bị cắt bên trái.
- [ ] Zoom 150% (**Ctrl+=** hai lần), lặp lại bước đầu.
  **Đúng:** số vẫn vừa cột.

---

## D19. Cột rộng ra khi gặp giá trị dài hơn lúc cuộn

Độ rộng cột lấy từ 64 dòng đầu, rồi rộng thêm (tới 360 px, và không bao giờ đổi cột người dùng đã tự kéo) khi một khối dòng về sau có giá trị dài hơn. Để cuộn vẫn nhanh, chỉ đo những ô dài hơn (tính theo byte) ô dài nhất đã đo trong cột đó; phần lớn khối không đo gì.

- [ ] Chạy câu của mục D18. Cột thứ ba (`'row ' || x`) lúc đầu vừa `row 1`…`row 64`. Cuộn xuống dòng 1.000, rồi 100.000, rồi 2.000.000.
  **Đúng:** cột rộng dần ra để `row 100000`, `row 2000000` hiện đủ, không bị cắt; khi đã đủ rộng thì không đổi nữa.
- [ ] Kéo cạnh tiêu đề cột thứ ba cho hẹp lại, rồi cuộn tiếp tới vùng có giá trị dài hơn.
  **Đúng:** cột giữ độ rộng đã kéo.
- [ ] Một cột chứa văn bản rất dài (ví dụ `select x, case when x = 5000 then printf('%.500c', 'a') else 'b' end from c` với `limit 10000`): cuộn tới dòng 5.000.
  **Đúng:** cột rộng tới 360 px là dừng; giá trị bị cắt ở 360 px như trước.
- [ ] Đo: F12 › Performance, ghi 5 giây cuộn liên tục (bánh xe và kéo thanh cuộn) trên câu của D18.
  **Đúng:** khung hình dưới 8 ms như `.claude/rules/data-grid.md` yêu cầu; không có tác vụ dài khi khối dòng về.

---

## D20. Số đang sửa canh phải như các số xung quanh

- [ ] Mở một bảng có cột số (ví dụ `int`, `decimal`). Nhấp đúp một ô số, gõ `12345`, Enter (chưa lưu).
  **Đúng:** ô tô màu sửa, `12345` canh phải như các số khác trong cột, không nhảy sang trái.
- [ ] Ở cùng cột, đặt một ô thành NULL (nút NULL trong editor).
  **Đúng:** `NULL` vẫn canh trái, chữ nghiêng nhạt như mọi NULL.
- [ ] Thêm một dòng mới vào bảng, gõ số vào cột số.
  **Đúng:** số canh phải; các cột chữ canh trái; ô chưa gõ ghi `DEFAULT` canh trái.
- [ ] Sửa một ô của cột chữ thành `123`.
  **Đúng:** canh trái (theo kiểu của ô, không theo nội dung).
