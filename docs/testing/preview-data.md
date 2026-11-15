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
