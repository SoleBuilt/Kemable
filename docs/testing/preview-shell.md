# Kiểm thử thủ công: các sửa lỗi trước bản preview (khung ứng dụng)

Cùng cách làm với `docs/testing/phase-1.md`: phần chuẩn bị (mục 0) và cách ghi kết quả giống hệt. Mỗi mục dưới đây ứng với một mục trong `docs/preview-audit.md` (cùng mã, ví dụ A7) và chỉ thử những gì máy không tự kiểm được.

---

## A7. Đóng cửa sổ khi còn việc dang dở

Đóng cửa sổ (nút X, Alt+F4, chuột phải trên taskbar › Close window) hỏi trước khi làm dừng một query đang chạy, một tab Live đang nghe, hoặc làm mất các sửa đổi bảng chưa lưu. Danh sách là danh sách bản cập nhật đã dùng (`docs/decisions/0028-updates.md`), trừ các trang `.sql` có thay đổi chưa lưu vào file: chúng được giữ lại như trước.

- [ ] Kết nối một SQLite, không làm gì thêm, nhấn **Alt+F4**.
  **Đúng:** cửa sổ đóng ngay, không hỏi gì.
- [ ] Mở lại, mở một bảng, sửa một ô (chưa nhấn Ctrl+S), bấm nút **X** của cửa sổ.
  **Đúng:** hộp thoại **Close Kemable?** hiện danh sách có dòng `<tên bảng> has table edits not saved: they are lost.`; cửa sổ vẫn mở.
- [ ] Nhấn **Esc** (hoặc **Back**).
  **Đúng:** hộp thoại đóng, sửa đổi vẫn còn (ô vẫn tô màu).
- [ ] Bấm **X** lần nữa, rồi **Close anyway** (hoặc Enter).
  **Đúng:** cửa sổ đóng. Mở lại: các trang query vẫn còn, vị trí cửa sổ đúng chỗ cũ, sửa đổi bảng không còn.
- [ ] Chạy một query dài (SQLite: `with recursive c(x) as (select 1 union all select x + 1 from c) select count(*) from c;`), trong lúc **Running**, chuột phải vào biểu tượng trên taskbar › **Close window**.
  **Đúng:** hộp thoại có dòng `A query is running: it stops.`. **Back** thì query vẫn chạy; Stop nó rồi đóng thì không hỏi nữa.
- [ ] Với PostgreSQL: mở tab Live (Ctrl+Alt+L), nghe `orders`, rồi **Alt+F4**.
  **Đúng:** hộp thoại có dòng `Live is listening: it stops.`.
- [ ] Mở một file `.sql`, gõ thêm vài chữ (chưa Ctrl+S), **Alt+F4**.
  **Đúng:** không hỏi; cửa sổ đóng. Mở lại: trang của file vẫn còn chữ vừa gõ và vẫn đánh dấu chưa lưu.
