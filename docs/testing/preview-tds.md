# Kiểm thử thủ công trước bản preview: SQL Server (TDS)

Những gì cần thử bằng tay sau các thay đổi ở client TDS của Kemable (`core/crates/tabstream`, ADR 0029). Phần tự động là các test live của driver SQL Server (`core/crates/drivers/mssql/src/tests.rs`, chạy với `KEMABLE_MSSQL_URL=sqlserver://localhost`); tài liệu này là phần người bấm mới kiểm được.

Viết bằng tiếng Việt vì đây là tài liệu cho người kiểm thử đọc, không phải quy tắc cho agent.

**Cách ghi kết quả:** đánh dấu `[x]` khi đúng. Khi sai, ghi ngay dưới mục đó: đã làm gì, thấy gì, mong đợi gì. Sau đó nhờ agent dùng skill `fix-issue`.

**Chuẩn bị:** SQL Server ở `localhost`, đăng nhập bằng Windows Authentication; chạy app bằng `pnpm tauri dev` trong `ui/windows`.

---

## A9. Stop giữ nguyên session SQL Server

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
