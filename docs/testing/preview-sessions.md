# Kiểm thử thủ công: phiên, kết nối lại và bộ nhớ (trước bản preview)

Cùng cách làm với `docs/testing/phase-1.md`: phần chuẩn bị (mục 0) và cách ghi kết quả giống hệt. File này gồm các mục của `docs/preview-audit.md` về phiên làm việc với server, kết nối lại và giới hạn bộ nhớ của một kết quả. Mỗi mục mang mã của nó trong bản audit.

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
