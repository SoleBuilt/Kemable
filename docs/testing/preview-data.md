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
