# Kiểm thử thủ công bổ sung

Các ca smoke test bổ sung, ngoài checklist theo phase.

## 2026-11-20 — In-memory SQLite: kết nối, Unicode, History và Run all

- **Trạng thái:** Đạt.
- **Đã làm:** Từ Welcome mở **Open an in-memory database**. Chạy `select 1 as one, 'xin chào' as greeting;`, mở History, rồi chạy `select 1 as one` và `select 2 as two` cách nhau bởi một dòng trống bằng **Run all**.
- **Thấy:** Toolbar ghi `In memory` và `SQLite 3.54.1`. Grid hiện `one = 1`, `greeting = xin chào`. History ghi câu vừa chạy với trạng thái thành công và `1 rows`. Run all tạo hai tab kết quả; status bar ghi `Ran 2 statements`.
- **Mong đợi:** Kết nối SQLite in-memory mở được; tiếng Việt không lỗi; query thành công được ghi vào History; hai câu cách nhau bằng dòng trống chạy thành hai kết quả riêng.
- **Kết quả:** Khớp mong đợi. Chạy trên app Kemable đã cài; không kết nối DB cloud.

## 2026-11-20 — Sidebar, SQL files và command palette

### Lọc Objects

- **Đã làm:** Nhập `kemable_no_match` vào Filter objects, sau đó đổi thành `thu_so` và cuối cùng xóa bộ lọc.
- **Thấy:** Với từ không khớp, cây báo `Nothing matches`; với `thu_so`, bảng xuất hiện lại. Xóa bộ lọc khôi phục danh sách.
- **Mong đợi:** Bộ lọc chỉ ẩn/hiện đối tượng theo nội dung đã nhập và có thể khôi phục danh sách.
- **Kết quả:** Khớp mong đợi.

### SQL files

- **Đã làm:** Mở mục SQL FILES, bấm **Link folder…**, rồi bấm **Cancel** trong hộp thoại chọn thư mục.
- **Thấy:** Mục mở ra với thông báo `No folder linked`; hộp thoại chọn thư mục mở được và Cancel quay lại app mà không liên kết thư mục.
- **Mong đợi:** Khi chưa có thư mục, app báo chưa liên kết; có thể mở hộp thoại và hủy mà không đổi cấu hình.
- **Kết quả:** Khớp mong đợi. Không chọn thư mục để tránh thay đổi cấu hình.

### Command palette và tab mới khi truy vấn đang chạy

- **Đã làm:** Mở Ctrl+K, bấm ô tìm kiếm và nhấn Esc; sau đó bấm **New query tab** và thử **Ctrl+T** khi Query 5 vẫn ở trạng thái chạy.
- **Thấy:** Esc đóng palette. Cả nút **New query tab** và Ctrl+T đều không tạo tab; Query 5 vẫn được chọn, status bar vẫn ghi `Running… · Ran 1 statement`, và nút Run vẫn bị vô hiệu hóa.
- **Mong đợi:** Esc đóng palette; nút New query tab không bị vô hiệu hóa và lệnh Ctrl+T được liệt kê thì cần mở tab mới.
- **Kết quả:** Palette đạt. Không mở được tab mới trong khi truy vấn mắc ở `Running…`; chưa thể kết luận hành vi của tính năng ở trạng thái bình thường.

- **Đã làm:** Bấm nút đóng trên tab Query 5 khi câu `SELECT` vẫn ở trạng thái `Running…`.
- **Thấy:** Query 5 vẫn được chọn, câu SQL vẫn trong editor, status bar còn `Running…` và nút Run vẫn bị vô hiệu hóa.
- **Mong đợi:** Đóng tab đang chạy sẽ hủy hoặc kết thúc truy vấn và trả ứng dụng về trạng thái sẵn sàng.
- **Kết quả:** Không thấy tab đóng hay trạng thái chạy thay đổi. Chưa thử lại để tránh lặp thao tác trên truy vấn đang treo.

### Style Orange trong Settings

- **Đã làm:** Mở Settings, đổi Style từ Black and white sang Orange mà không lưu, rồi bấm Cancel.
- **Thấy:** Nút Run/Save và checkbox chuyển cam ngay. Sau Cancel, Settings đóng và giao diện trở lại đen trắng.
- **Mong đợi:** Accent cam áp dụng ngay trong lúc xem trước; Cancel khôi phục style ban đầu.
- **Kết quả:** Đạt với các nút và checkbox quan sát được. Không có grid kết quả do query còn `Running…`, nên chưa kiểm tra được màu con trỏ ô, con trỏ gõ hoặc gạch dưới tab. Khi tiếp tục mở Settings, cửa sổ chính lại mất khả năng truy cập qua Computer Use; danh sách chỉ còn cửa sổ `kemable.app-siw` kích thước 16 × 16. Không khởi động lại app.

### Settings, Mica và kiểm tra cập nhật

- **Đã làm:** Mở command palette, tìm `Settings`, chọn kết quả rồi nhấn Enter. Trong Settings bật Mica, nhấn **Check now**, sau đó bấm **Cancel** để không lưu thiết lập.
- **Thấy:** Nhấn Enter mở Settings. Radio Mica chuyển sang **On**; ảnh chụp không cho thấy rõ nền desktop xuyên mờ. **Check now** tạm thời bị vô hiệu hóa rồi hoạt động lại; không thấy thông báo hay trạng thái kết quả mới. Cancel đóng Settings. Query 5 vẫn `Running…`.
- **Mong đợi:** Mica làm toolbar/sidebar thấy nền desktop mờ khi đặt cửa sổ trước nền có màu; Check now đưa ra trạng thái cập nhật dễ nhận biết; Cancel bỏ thiết lập chưa lưu.
- **Kết quả:** Cancel khôi phục thiết lập chưa lưu. Mica và kết quả kiểm tra cập nhật chưa kết luận được do không quan sát rõ hiệu ứng hoặc phản hồi sau khi nhấn nút.
