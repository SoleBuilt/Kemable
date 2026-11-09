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

## 3. Duyệt bảng (table tab): phân trang, lọc, sort trên server

Dùng `demo_sales` (PostgreSQL) và một bảng bất kỳ ở SQL Server. Bảng `order_items` có khoảng 10.000 dòng, hợp để thử phân trang.

- [ ] Double-click `order_items` ở tab **Objects**.
  **Đúng:** trên kết quả có thanh bảng: nút **Filter**, tên `public.order_items`, ghi `1–1,000 of …` rồi ngay sau đó `of 10,000` (hoặc số dòng thật). Grid hiện 1.000 dòng, xếp theo khóa chính (`id` tăng dần).
- [ ] Bấm **▶** (hoặc **Alt+PageDown**), rồi **◀** (**Alt+PageUp**), **⏭**, **⏮**.
  **Đúng:** ghi `1,001–2,000`, quay lại `1–1,000`; ⏭ tới trang cuối (ví dụ `10,001–10,0xx`), ⏮ về đầu. Không dòng nào lặp giữa hai trang liền nhau.
- [ ] Đổi **/ page** sang `100`.
  **Đúng:** mỗi trang 100 dòng; tắt mở lại app thì vẫn nhớ 100.
- [ ] Bấm tiêu đề cột `quantity` (hoặc cột số bất kỳ) một lần, hai lần, ba lần.
  **Đúng:** lần 1 tăng dần, lần 2 giảm dần, lần 3 bỏ sort; mỗi lần quay về trang 1, và dòng đầu của trang 1 là giá trị lớn/nhỏ nhất **của cả bảng**, không chỉ của trang đang xem. Status bar ghi "sorted by the server".
- [ ] Nhấn **Ctrl+F** khi grid hoặc sidebar đang được chọn (không phải trong editor).
  **Đúng:** khung lọc mở, con trỏ ở ô giá trị đầu. (Trong editor, Ctrl+F vẫn là tìm chữ của editor.)
- [ ] Lọc `customers`: cột `full_name` (hoặc cột tên), **contains**, gõ một phần tên viết thường, nhấn **Enter**.
  **Đúng:** chỉ còn các dòng có chữ đó, không phân biệt hoa thường; nút ghi **Filter (1)**; tổng đếm lại.
- [ ] Thêm filter thứ hai bằng **+ Add filter**, chọn **Match any**, Apply.
  **Đúng:** kết quả là hợp của hai điều kiện; đổi lại **Match all** thì là giao.
- [ ] Thử từng loại: `=`, `≠`, `>`, `≤`, **in** (`1, 2, 3`), **between** (hai ô from/to), **is NULL**, **is not NULL**, **starts with**, **ends with**, **does not contain**.
  **Đúng:** mỗi loại cho kết quả đúng nghĩa của nó.
- [ ] Gõ giá trị có ký tự đặc biệt: `%`, `_`, `'`, ví dụ contains `50%` hoặc `=` `O'Brien`.
  **Đúng:** tìm đúng chữ đó, `%` và `_` không thành ký tự đại diện; không có lỗi cú pháp.
- [ ] Chọn cột **SQL condition…**, gõ `quantity > 5 and unit_price < 100` (đổi tên cột theo bảng), Enter.
  **Đúng:** lọc theo đúng điều kiện. Gõ `1=1; drop table x` thì status bar báo lỗi "Not filtered…" và không chạy gì.
- [ ] Lọc cột số bằng chữ, ví dụ `id` `=` `abc` trên PostgreSQL.
  **Đúng:** tab Messages hiện lỗi của server (kiểu dữ liệu sai); thanh bảng và các filter vẫn còn để sửa.
- [ ] Bấm **Clear** trong khung lọc.
  **Đúng:** bỏ hết filter, về trang 1, đếm lại tổng.
- [ ] Bấm **Copy SQL**, dán vào editor và chạy.
  **Đúng:** câu `SELECT * FROM ... WHERE ... ORDER BY ... LIMIT ... OFFSET ...` chạy ra đúng trang đang xem. Trên SQL Server là `OFFSET ... ROWS FETCH NEXT ... ROWS ONLY`.
- [ ] Bấm **Refresh** sau khi chèn thêm một dòng vào bảng bằng editor.
  **Đúng:** tổng tăng 1, dòng mới hiện ra ở đúng vị trí.
- [ ] Mở một bảng nhỏ (ít hơn 1 trang, ví dụ `categories`).
  **Đúng:** tổng hiện ngay (không có bước "of …"), nút ▶ và ⏭ bị mờ.
- [ ] Mở một view.
  **Đúng:** vẫn phân trang được; thứ tự khi chưa sort có thể không cố định (view không có khóa chính).
- [ ] Chạy một câu bất kỳ từ editor khi đang ở table tab.
  **Đúng:** thanh bảng biến mất, kết quả là của câu vừa chạy.
- [ ] Mở bảng, kiểm tra tab **History**.
  **Đúng:** không có câu nào của table tab trong lịch sử.
- [ ] Làm lại các bước chính trên **SQL Server** và trên một file **SQLite**.
  **Đúng:** như trên.

Test tự động cho phần này: `a_table_tab_pages_and_filters_on_sqlite` chạy trong `node scripts/verify.mjs`; bản PostgreSQL và SQL Server chạy cùng các test live ở cuối `docs/testing/phase-1.md`.
