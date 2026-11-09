# Kiểm thử thủ công Phase 2

Cùng cách làm với `docs/testing/phase-1.md`: phần chuẩn bị (mục 0) và cách ghi kết quả giống hệt. File này chỉ thêm những gì Phase 2 mang lại. Mỗi tính năng xong sẽ thêm mục của nó vào đây.

---

## 1. Lịch sử truy vấn (History)

Lịch sử nằm trong `history.db` ở thư mục dữ liệu của app (`%APPDATA%\<app id>`), giữ tối đa 20.000 câu, câu cũ nhất bị xóa trước.

- [ ] Kết nối `Local PG`, chạy `select now();` bằng **Ctrl+Enter**, rồi mở tab **History** ở sidebar.
  **Đúng:** câu vừa chạy đứng đầu, có dấu ✓, giờ chạy, số ms, `1 rows`, tên database.
- [ ] Chạy một script 3 câu bằng **Ctrl+Shift+Enter**.
  **Đúng:** History có 3 mục, mỗi câu một mục, câu chạy sau nằm trên.
- [ ] Chạy `select * from bang_khong_co;`.
  **Đúng:** mục mới có dấu ✕ màu đỏ.
- [ ] Chạy `select pg_sleep(30);`, bấm **Stop**.
  **Đúng:** mục mới có dấu ■ (đã dừng).
- [ ] Double-click một bảng ở tab **Objects**, bấm tiêu đề một cột để sort, bấm **Fetch all** nếu có.
  **Đúng:** History **không** thêm mục nào: đây là câu app tự viết, không phải câu người dùng gõ.
- [ ] Chạy `alter user postgres password 'abc';` trên một user thử (hoặc gõ rồi chạy bất kỳ câu nào chứa chữ `PASSWORD`, kể cả câu lỗi).
  **Đúng:** mục mới ghi `-- not kept: this statement holds a password`, không lưu câu gốc.
- [ ] Gõ `now` vào ô **Search history**.
  **Đúng:** sau khoảng 0,2 giây chỉ còn những câu chứa `now`.
- [ ] Bỏ tick **This connection**, rồi kết nối `Local MSSQL` và mở History.
  **Đúng:** khi tick thì chỉ thấy câu của SQL Server; bỏ tick thì thấy cả câu của PostgreSQL.
- [ ] Rê chuột lên một mục, bấm **Insert**.
  **Đúng:** câu được chèn vào editor ở vị trí con trỏ, trên một dòng riêng, và được bôi chọn. Double-click mục cũng làm như vậy.
- [ ] Bấm **Run** trên một mục chạy ở database khác (ví dụ chạy ở `postgres` khi đang ở `demo_sales`).
  **Đúng:** toolbar chuyển sang database của mục đó, câu chạy và có kết quả, History thêm một mục mới.
- [ ] Bấm **Delete** trên một mục; bấm **Clear** rồi **Yes**.
  **Đúng:** mục đó mất; sau Clear thì danh sách trống và ghi "No queries yet…".
- [ ] Chạy hơn 100 câu (ví dụ dán 120 dòng `select 1;` rồi Run all), cuộn xuống cuối History.
  **Đúng:** có nút **Load more**, bấm thì hiện tiếp các câu cũ hơn.
- [ ] Tắt app, mở lại, kết nối lại.
  **Đúng:** lịch sử vẫn còn.

## 2. Truy vấn đã lưu (Queries)

Lưu trong `queries.json` cạnh `connections.json`.

- [ ] Gõ một câu vào editor, nhấn **Ctrl+Shift+D** (hoặc bấm **Save query** trên toolbar).
  **Đúng:** hộp thoại mở, tên là dòng đầu của câu, ô SQL có câu đó, con trỏ ở ô Name.
- [ ] Bôi chọn một phần của editor rồi nhấn **Ctrl+Shift+D**.
  **Đúng:** ô SQL chỉ có phần đã chọn.
- [ ] Đặt tên `Doanh thu theo tháng`, để **Show it for every connection** không tick, bấm **Save**.
  **Đúng:** status bar ghi "Query saved"; tab **Queries** có mục mới.
- [ ] Lưu thêm một câu có tick **Show it for every connection**, rồi kết nối `Local MSSQL`.
  **Đúng:** ở SQL Server chỉ thấy câu tick "every connection" (có chữ nhỏ `all connections`), không thấy câu riêng của PostgreSQL.
- [ ] Kết nối **In memory** (kết nối không lưu), nhấn **Ctrl+Shift+D**.
  **Đúng:** ô tick bị khóa và ghi "(this connection is not saved)"; câu lưu ra dùng cho mọi kết nối.
- [ ] Trong tab Queries: **Insert**, **Run**, **Edit** (đổi tên, sửa SQL, Save), **Delete** rồi **Yes**.
  **Đúng:** Insert chèn vào editor; Run chạy câu và ghi vào History; Edit lưu đè đúng mục đó, không tạo mục mới; Delete xóa mục.
- [ ] Gõ vào ô **Filter saved queries**.
  **Đúng:** lọc theo cả tên và nội dung SQL, không phân biệt hoa thường.
- [ ] Nhấn **Ctrl+Shift+D** khi editor trống.
  **Đúng:** không mở hộp thoại; status bar ghi "Nothing to save: the editor is empty".
- [ ] Nhấn **Esc** trong hộp thoại.
  **Đúng:** hộp thoại đóng, con trỏ về editor.
- [ ] Chuyển qua lại giữa Objects, Queries, History.
  **Đúng:** cây ở Objects giữ nguyên các nút đã mở.
