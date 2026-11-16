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

## D10. PostgreSQL: Stop không bị chặn sau một lần dừng không trọn

Khi một lần chạy bị dừng mà lệnh huỷ không tới server, lần chạy sau phải dọn transaction cũ trước. Việc dọn đó giờ nằm trong vùng mà Stop theo dõi.

- [ ] Kết nối PostgreSQL, chạy `select pg_sleep(60);` rồi nhấn Esc ngay.
  **Đúng:** báo **Stopped** trong khoảng một giây.
- [ ] Chạy `select * from generate_series(1, 5000000);` (giới hạn 1,000 dòng), nhấn Esc khi còn "Running"; ngay sau đó chạy `select pg_sleep(30);` và nhấn Esc.
  **Đúng:** lần thứ hai cũng dừng trong khoảng một giây, không phải chờ.

## D11. RabbitMQ: publish chờ broker xác nhận

- [ ] Mở console RabbitMQ, chạy `publish khong_co_exchange x hello`.
  **Đúng:** báo **The query failed** kèm "NOT_FOUND - no exchange 'khong_co_exchange'", không phải "Done".
- [ ] Ngay sau đó chạy `publish amq.topic test.x hello`.
  **Đúng:** chạy được (channel bị broker đóng đã được mở lại).
- [ ] Chạy `publish amq.direct khong_ai_nghe hello` (không queue nào bind khoá này).
  **Đúng:** Done, kèm một dòng ở Messages: "The broker returned the message: no queue is bound to take it."

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
