# Kiểm thử thủ công: các lỗi dữ liệu trước bản preview

Cùng cách làm với `docs/testing/phase-1.md`: phần chuẩn bị (mục 0) và cách ghi kết quả giống hệt. File này kiểm những lỗi về dữ liệu tìm thấy trong `docs/preview-audit.md`, mỗi mục một phần, đặt theo mã của mục đó.

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
