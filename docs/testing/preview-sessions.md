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

## B11. SQLite: gợi ý và cây không dừng câu đang chạy

- [ ] Mở SQLite `:memory:`, chạy một câu dài (khoảng 10 giây):
  ```sql
  with recursive c(x) as (select 1 union all select x + 1 from c limit 100000000) select count(*) from c;
  ```
  Trong lúc nó chạy, mở tab query khác, gõ `select * from ` để hiện gợi ý, và mở rộng cây ở sidebar.
  **Đúng:** gợi ý và cây có thể chưa ra (chờ câu kia), nhưng câu dài vẫn chạy tới hết và ra `100000000`; không báo "The server took too long".
- [ ] Chạy lại câu dài, bấm **Stop** (Esc) ngay sau khi bấm Run.
  **Đúng:** dừng trong khoảng một giây, báo **Stopped**.

## B12. Mở kết nối khác khi một câu đang chạy

Khi một câu đang chạy, mở kết nối khác bị từ chối và nhắc dừng câu trước, như Disconnect. Đóng kết nối thì câu của nó dừng theo. Sự kiện của một lần chạy cũ không bao giờ hiện trên trang của kết nối mới.

- [ ] Kết nối PostgreSQL, chạy `select pg_sleep(30);`. Trong lúc nó chạy, mở một kết nối đã lưu khác (Ctrl+K).
  **Đúng:** kết nối mới không mở; dải lỗi ghi "Stop the running query first (Esc)". Trang vẫn hiện câu đang chạy.
- [ ] Nhấn **Esc** để dừng, rồi mở lại kết nối kia.
  **Đúng:** mở được; trang mới không có kết quả, lỗi hay chữ "finished" nào của lần chạy cũ.
- [ ] SQL Server: chạy `waitfor delay '00:00:30';`, nhấn Esc, mở ngay kết nối khác.
  **Đúng:** kết nối mới mở trong vài giây, không đứng ở "Running".

## D8. Mở phiên của một database không làm đứng cả kết nối

Phiên của một database mở trong một tác vụ riêng: các database khác vẫn dùng được trong lúc chờ, và Stop hoặc hạn giờ của lời gọi kết thúc việc chờ.

- [ ] Kết nối PostgreSQL qua một mạng chậm, hoặc một Neon project đang ngủ (mất vài giây để thức). Trong cây, mở một database chưa mở lần nào; ngay lúc đó mở rộng database home và chạy `select 1;` ở tab query trên home.
  **Đúng:** cây của home và câu `select 1` ra ngay, không phải chờ database kia mở xong.
- [ ] Chọn database đang mở chậm ở thanh công cụ, chạy `select 1;`, nhấn **Esc** khi còn đang "Running".
  **Đúng:** dừng ngay, báo **Stopped**; chạy lại sau vài giây thì chạy được trên phiên đã mở xong.
