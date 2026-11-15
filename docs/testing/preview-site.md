# Kiểm thử thủ công: website, README, ký installer, ghi chú phát hành

Các mục phát hành và website trước bản preview đầu tiên (`docs/preview-audit.md`, C17, C18, C19, D16). Mỗi mục một phần. Đánh dấu `[x]` khi đúng; khi sai, ghi ngay dưới dòng đó thấy gì.

---

## C17. Website của Kemable (`site/`) và đường dẫn Terms, Privacy trong app

`docs/decisions/0030-website.md`. Cần Node 22+ và pnpm.

### C17.1 Chạy trên máy

- [ ] Trong `site/`: `pnpm install`, rồi `pnpm build`.
  **Đúng:** build xong trong vài giây, không cần mạng (ngoài lúc cài); có `site/dist/index.html`, `terms.html`, `privacy.html`.
- [ ] `pnpm preview`, mở `http://localhost:4321`.
  **Đúng:** trang chủ nền tối, giống bản thiết kế: thanh trên có icon Kemable, Features, Engines, GitHub, nút Download; dòng "Fast is the feature."; video demo tự chạy, lặp, không tiếng; dòng engine; ba con số 2 ms, 1.07 s, 0.13 s; ba phần Edit, Ctrl K, Light or dark với ảnh; bốn thẻ nhỏ; khối "Open your data in seconds."; chân trang có Terms, Privacy, GitHub và nút đổi giao diện.
- [ ] Tab trình duyệt.
  **Đúng:** favicon là icon Kemable (ô vuông cam giữa hai góc trắng).
- [ ] Bấm **Features**, **Engines** trên thanh trên.
  **Đúng:** cuộn tới phần tương ứng của trang chủ.
- [ ] Bấm **Download**, **Download for Windows**, chip **New**.
  **Đúng:** mở `https://github.com/SoleBuilt/Kemable/releases/latest`.
- [ ] Bấm **SoleBuilt** ở chân trang.
  **Đúng:** mở `https://solebuilt.ducht.info.vn/`.
- [ ] Cuộn xuống chân trang, bấm nút mặt trời.
  **Đúng:** cả trang chuyển sang nền trắng ngay; nút thành mặt trăng. Tải lại trang: vẫn sáng, không chớp tối lúc tải. Bấm lại: về tối.
- [ ] Thu cửa sổ trình duyệt hẹp như điện thoại (hoặc F12, chế độ thiết bị, 375 px).
  **Đúng:** không có thanh cuộn ngang; thanh trên chỉ còn icon, Kemable và Download; ảnh nằm dưới chữ.
- [ ] Mở `/terms` và `/privacy` (từ chân trang).
  **Đúng:** đầu mỗi trang có khung "Draft for legal review."; nội dung nói đúng app làm gì: không telemetry, một lần hỏi `latest.json` của GitHub Releases lúc mở app, dữ liệu trong thư mục data trên máy, mật khẩu trong Windows Credential Manager, kết nối chỉ tới nơi người dùng chỉ định, AGPL và license thương mại.
- [ ] F12, tab Network, tải lại trang chủ.
  **Đúng:** mọi yêu cầu đều tới `localhost` (font cũng vậy), không có Google Fonts hay analytics; không file `.js` nào tải trước khi cuộn tới chân trang.

### C17.2 Triển khai lên Cloudflare Pages

Làm theo `site/README.md` (Deploying) một lần: tạo project Pages `kemable`, thêm hai secret.

- [ ] Trước khi có secret: merge một thay đổi trong `site/` vào `main` (hoặc Actions › Site › Run workflow).
  **Đúng:** job **site** xanh; có thông báo "Built, not deployed. Add the CLOUDFLARE_API_TOKEN and CLOUDFLARE_ACCOUNT_ID secrets".
- [ ] Sau khi thêm `CLOUDFLARE_API_TOKEN` và `CLOUDFLARE_ACCOUNT_ID`: chạy lại workflow trên `main`.
  **Đúng:** bước **Deploy to Cloudflare Pages** chạy, in địa chỉ deploy; `https://kemable.pages.dev` hiện trang chủ.
- [ ] Mở `https://kemable.pages.dev/terms` và `https://kemable.pages.dev/privacy`.
  **Đúng:** mở thẳng trang, không chuyển hướng sang `/terms/`.
- [ ] Một pull request chỉ sửa `site/`.
  **Đúng:** job **site** chạy check và build, không deploy.

### C17.3 Trong app

- [ ] `pnpm tauri dev`, mở app lần đầu với thư mục data trống.
  **Đúng:** không có hộp thoại Terms (vẫn ẩn, như trước).
- [ ] Tìm trong mã nguồn và ADR tên sản phẩm cũ dùng làm trang tạm: `git grep -i -n "clause/terms"`.
  **Đúng:** không còn kết quả; `open_terms` trong `commands.rs` chỉ biết `https://kemable.pages.dev/terms` và `/privacy`.

---

## C18. README tiếng Anh, bản tiếng Việt bên cạnh

- [ ] Mở trang repository trên GitHub.
  **Đúng:** README bằng tiếng Anh: Kemable là gì, ảnh chụp app (giao diện sáng) hiện ra, phần Download dẫn tới `releases/latest`, bảng engine, Building from source, License.
- [ ] Bấm **Tiếng Việt** ở đầu README.
  **Đúng:** mở `README.vi.md`; ở đầu có **English** dẫn ngược về `README.md`.
- [ ] Làm theo phần "Chạy từ mã nguồn" của `README.vi.md` trên một máy sạch.
  **Đúng:** màn hình Welcome có đúng các nút README nhắc (**Open an in-memory database**, **Open file…**, **New connection…**); câu SQL 2 triệu dòng chạy được.
