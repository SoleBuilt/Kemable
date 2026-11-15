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

## B13. Báo lỗi khi ứng dụng không mở được

Khi bước khởi động lỗi (thiếu WebView2, không tạo được thư mục dữ liệu), ứng dụng hiện một hộp thoại của Windows ghi lý do rồi mới thoát, thay vì tắt im lặng. Thư mục WebView của mỗi người dùng Windows là của riêng họ: `%LOCALAPPDATA%\<identifier>\webview`, không còn là `data\webview`.

- [ ] Đóng Kemable bản dev. Đổi tên thư mục `%APPDATA%\dev.kemable.app` thành `dev.kemable.app.bak`, rồi tạo một **file** rỗng tên `dev.kemable.app` ở đúng chỗ đó. Chạy `pnpm tauri dev`.
  **Đúng:** một hộp thoại lỗi của Windows tiêu đề **Kemable could not start**, có dòng lỗi về thư mục; bấm **OK** thì ứng dụng thoát. Xoá file rỗng, đổi tên thư mục `.bak` về như cũ.
- [ ] Mở Kemable bình thường, xem `%LOCALAPPDATA%\dev.kemable.app\webview` (bản cài: `%LOCALAPPDATA%\kemable.app\webview`).
  **Đúng:** thư mục có, chứa `EBWebView`; trong thư mục dữ liệu (`data\` cạnh exe, hoặc `%APPDATA%\dev.kemable.app`) không có thư mục `webview` mới được tạo.
- [ ] Bản cài cho mọi người (Everyone) trên máy có hai tài khoản Windows: tài khoản thứ nhất mở Kemable và để đó, chuyển sang tài khoản thứ hai (Switch user), mở Kemable.
  **Đúng:** ứng dụng mở được ở cả hai tài khoản, cùng danh sách kết nối đã lưu.

## B14. `connections.json` hỏng

File kết nối đã lưu không đọc được thì ứng dụng nói rõ, kèm đường dẫn file và nút mở thư mục, thay vì hiện như chưa lưu kết nối nào. Lưu kết nối vẫn từ chối ghi đè lên file đó.

- [ ] Đóng Kemable bản dev. Sao lưu `%APPDATA%\dev.kemable.app\connections.json`, rồi mở file bằng Notepad, xoá dấu `}` cuối cùng, lưu lại. Mở Kemable.
  **Đúng:** màn hình Welcome có dải đỏ **Saved connections: A file Kemable keeps cannot be read; it is left as it is**, dòng dưới ghi `cannot understand ...\connections.json: ...`; có nút **Open its folder**.
- [ ] Bấm **Open its folder**.
  **Đúng:** Explorer mở đúng thư mục chứa `connections.json`.
- [ ] Nhấn **Ctrl+K**.
  **Đúng:** nhóm **Saved connections** có một dòng **Saved connections cannot be read** (biểu tượng cảnh báo), không phải danh sách trống; chọn nó thì mở thư mục.
- [ ] Nhấn **Ctrl+N**, chọn PostgreSQL.
  **Đúng:** form hiện ngay lỗi đỏ cùng nội dung. Điền đủ, bấm **Connect** (có lưu): không lưu được, báo lỗi; file `connections.json` vẫn y như lúc bị sửa (không bị ghi đè).
- [ ] Đóng Kemable, chép file sao lưu về chỗ cũ, mở lại.
  **Đúng:** không còn dải đỏ; Ctrl+K có lại các kết nối đã lưu.

## B15. Mật khẩu đã lưu chỉ gửi tới nơi nó được lưu cho

Mật khẩu (hoặc token) giữ trong Credential Manager gắn với engine, host, port, user và SSH server của kết nối lúc lưu (`docs/decisions/0033-bound-passwords.md`). Khi `connections.json` bị đổi các thứ đó (ví dụ bởi một người dùng Windows khác trên cùng máy), Kemable không gửi mật khẩu đã lưu mà hỏi lại. `known_hosts` của Kemable nằm trong thư mục riêng của từng người dùng Windows.

- [ ] Lưu một kết nối PostgreSQL `Local PG` có **Save password**. Mở nó qua Ctrl+K.
  **Đúng:** kết nối ngay, không hỏi mật khẩu (mật khẩu lưu từ trước bản này được gắn với kết nối như đang lưu, một lần).
- [ ] Đóng Kemable. Mở `connections.json` (thư mục dữ liệu) bằng Notepad, đổi `"host"` của `Local PG` thành `127.0.0.1` (khác chữ `localhost` đã lưu), lưu lại. Mở Kemable, mở `Local PG`.
  **Đúng:** hộp thoại **Password for Local PG** ghi `This connection now points to postgres@127.0.0.1:5432, not where its password was kept for...`; chưa có kết nối nào tới server (xem `select * from pg_stat_activity` từ một client khác nếu muốn chắc).
- [ ] Gõ đúng mật khẩu, **Connect**. Ngắt kết nối, mở lại `Local PG`.
  **Đúng:** lần đầu kết nối được; lần sau không hỏi nữa (mật khẩu được giữ cho host mới).
- [ ] Đổi `"host"` trong file về `localhost`, mở lại.
  **Đúng:** hỏi lại một lần (mật khẩu giờ gắn với `127.0.0.1`); gõ xong thì lần sau không hỏi.
- [ ] Mở form sửa kết nối (menu kết nối › Edit), đổi Port thành `5433`, để trống ô Password, bấm **Save**. Đổi lại `5432`, **Save**, rồi mở kết nối.
  **Đúng:** không hỏi mật khẩu: sửa trong form của chính mình thì mật khẩu đi theo.
- [ ] Với một kết nối qua SSH tunnel có lưu mật khẩu SSH: sửa `"ssh"` › `"host"` trong `connections.json` sang một máy khác, mở kết nối.
  **Đúng:** dải đỏ **This connection's SSH server changed since its SSH password was kept: edit the connection to enter it again**, có nút **Edit connection** mở form; không có kết nối SSH nào được mở.
- [ ] Bản cài cho mọi người: kết nối qua SSH tới một server mới, bấm **Trust this key and connect**. Xem `%APPDATA%\kemable.app\known_hosts` và `data\known_hosts` cạnh exe.
  **Đúng:** khoá nằm trong `%APPDATA%\kemable.app\known_hosts`; `data\known_hosts` không được ghi.

## C16. Bộ cài mang theo văn bản giấy phép

Bộ cài đặt `LICENSE.txt` (văn bản AGPL-3.0) cạnh `Kemable.exe`; Settings › About › **License** mở nó bằng Notepad, như Third-party notices, không cần mạng hay GitHub.

- [ ] Build bộ cài (`pnpm tauri build`), cài, rồi xem thư mục cài.
  **Đúng:** có `LICENSE.txt` và `THIRD-PARTY-NOTICES.md` cạnh `Kemable.exe`; `LICENSE.txt` bắt đầu bằng `GNU AFFERO GENERAL PUBLIC LICENSE`.
- [ ] Rút mạng, mở **Settings** (Ctrl+,) › About › **License**.
  **Đúng:** Notepad mở `LICENSE.txt` trong thư mục cài; không mở trình duyệt.
- [ ] Bản dev (`pnpm tauri dev`): bấm **License**.
  **Đúng:** Notepad mở `LICENSE.txt` từ thư mục build (`target\debug`).
