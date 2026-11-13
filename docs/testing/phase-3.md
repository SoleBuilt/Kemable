# Kiểm thử thủ công Phase 3

Cùng cách làm với `docs/testing/phase-1.md`: phần chuẩn bị (mục 0) và cách ghi kết quả giống hệt. File này chỉ thêm những gì Phase 3 mang lại. Mỗi tính năng xong sẽ thêm mục của nó vào đây.

---

## 1. Tab Live với PostgreSQL (`LISTEN` / `NOTIFY`)

Tab Live nghe trên một kết nối riêng tới server, mỗi sự kiện là một dòng trong grid (`docs/decisions/0020-live-tab.md`). Cách thử chung: nghe ở tab Live, gửi từ một tab query.

**Channel là gì.** `NOTIFY ten_kenh, 'noi dung'` phát một tin lên kênh `ten_kenh`. Server chỉ chuyển tin tới các kết nối đã `LISTEN` đúng kênh đó, trong cùng database. Tab Live nghe kênh nào thì chỉ nhận tin của kênh ấy: nghe `orders` thì tin gửi lên `khac` không tới, giống như radio chỉ bắt đài đang dò. Tên kênh không có nháy kép được PostgreSQL đổi về chữ thường (`Orders` thành `orders`); tên trong nháy kép giữ nguyên (`"Orders"` khác `orders`).

**Gửi nhiều tin bằng một câu chỉ trả về một dòng**, ví dụ `select count(pg_notify(...)) from generate_series(...)`. Câu `select pg_notify(...) from generate_series(1, 5000)` trả về 5.000 dòng: Run dừng ở giới hạn dòng (mặc định 1.000) và huỷ câu trên server. Khi câu chưa chạy xong mà đã bị huỷ, PostgreSQL bỏ cả câu, kèm mọi tin trong đó.

**Một transaction còn mở giữ tin lại.** PostgreSQL chỉ gửi tin lúc `COMMIT`. Nếu trong tab query đã chạy `BEGIN` mà chưa `COMMIT`, thì `NOTIFY` vẫn báo thành công nhưng chưa ai nhận. Kiểm bằng `select now() = statement_timestamp() as khong_co_transaction_mo;`: nếu ra `false` thì đang ở trong transaction, chạy `COMMIT;` (hoặc `ROLLBACK;`).

### 1.1 Mở tab và nghe

- [x] Kết nối `Local PG`, nhấn **Ctrl+Alt+L**.
  **Đúng:** một tab mới tên **Live** với biểu tượng sóng; không có editor; có một ô **Channels** (không có ô Patterns); vùng dưới ghi "Nothing heard yet" kèm gợi ý `NOTIFY orders, 'hello'`; con trỏ nằm sẵn trong ô Channels.
- [x] Mở **Ctrl+K**, gõ `live`.
  **Đúng:** có lệnh **New Live tab** với phím `Ctrl Alt L`. Kết nối SQLite hoặc SQL Server thì **không** có lệnh này, và Ctrl+Alt+L không làm gì.
- [x] Để ô Channels trống.
  **Đúng:** nút **Listen** bị mờ; nhấn F5 không làm gì.
- [x] Gõ `orders` vào ô Channels, nhấn **Enter** (hoặc **F5**, hoặc bấm **Listen**).
  **Đúng:** status bar ghi **Listening** và `0 events`, kèm `in <tên database>`; nút Listen mờ đi, nút **Stop** sáng lên; ô Channels bị khóa; grid hiện 4 cột `received`, `channel`, `payload`, `pid` và chưa có dòng nào.

### 1.2 Nhận sự kiện

- [x] Nhấn **Ctrl+T** mở một tab query, chạy `NOTIFY orders, 'xin chào';`, rồi quay lại tab Live.
  **Đúng:** chuyển tab được ngay dù tab Live đang nghe (không bị chặn như khi một query đang chạy). Tab Live có 1 dòng: `received` là giờ UTC lúc nhận, `channel` là `orders`, `payload` là `xin chào` (đúng dấu tiếng Việt), `pid` là một số. Nhãn tab Live hiện số `1`.
- [x] Ở tab query chạy `select count(pg_notify('orders', g::text)) from generate_series(1, 5000) g;`, quay lại tab Live.
  **Đúng:** câu trả về một dòng `5000`; tab Live có 5.001 sự kiện; status bar và nhãn tab ghi đúng số; cuộn grid mượt; dòng cuối có payload `5000`.
- [x] Chạy `NOTIFY Orders, 'chữ hoa';` (không có dấu nháy kép quanh tên).
  **Đúng:** tab Live **có** nhận: tên không nháy được PostgreSQL đổi về chữ thường, cả khi nghe lẫn khi gửi, nên cả hai đều là `orders`.
- [ ] Chạy `NOTIFY "Orders", 'nháy kép';`.
  **Đúng:** tab Live **không** nhận: tên trong nháy kép giữ nguyên là `Orders`, khác kênh `orders` đang nghe. Muốn nghe nó thì gõ `"Orders"` (có nháy kép) vào ô Channels.
- [x] Chạy `NOTIFY khac, 'x';`.
  **Đúng:** tab Live **không** thêm dòng nào: `khac` là kênh khác, tab chỉ nghe `orders`.
- [ ] Chạy một câu bất kỳ không tạo hay xoá bảng (`NOTIFY`, `INSERT`, `SELECT`), nhìn sidebar.
  **Đúng:** cây đối tượng đứng yên, không chớp. Chạy `create table thu_live (a int);` rồi `drop table thu_live;`: cây cập nhật mà không hiện "Loading…".

### 1.3 Follow, filter, sort, chi tiết dòng

- [x] Để **Follow** bật (mặc định), gửi thêm vài `NOTIFY`.
  **Đúng:** grid tự cuộn để dòng mới nhất luôn nằm trong tầm nhìn.
- [x] Bấm **Follow** để tắt, cuộn lên đầu, gửi thêm vài `NOTIFY`.
  **Đúng:** grid đứng yên ở chỗ đang xem; số sự kiện vẫn tăng.
- [x] Nhấn **Ctrl+F** (hoặc bấm **Filter**), chọn cột `payload`, điều kiện **contains**, giá trị `99`, bấm **Apply**.
  **Đúng:** grid chỉ còn các dòng payload chứa `99`; status bar ghi `<n> of <tổng> events`; nút Filter có số `1`.
- [ ] Trong lúc filter đang bật và tab vẫn đang nghe, chạy `NOTIFY orders, '1990';` rồi `NOTIFY orders, 'abc';`.
  **Đúng:** dòng `1990` xuất hiện, dòng `abc` thì không; tổng tăng 2, số đang hiện tăng 1.
- [x] Bấm **Clear** trong hàng filter.
  **Đúng:** hiện lại mọi sự kiện.
- [x] Bấm tiêu đề cột `payload` một lần, hai lần, ba lần.
  **Đúng:** tăng dần, giảm dần, rồi về thứ tự nhận; khi đang sort status bar ghi "sorted in Kemable" và grid không tự cuộn theo dòng mới.
- [ ] Bấm tiêu đề cột `received` hai lần (giảm dần, mũi tên chỉ xuống), rồi gửi `NOTIFY orders, 'mới nhất';`.
  **Đúng:** trong vòng nửa giây, dòng `mới nhất` lên **đầu** grid. Một lần bấm (tăng dần) thì dòng mới nằm **cuối**.
- [ ] Bấm một ô, nhấn **Ctrl+Alt+I**.
  **Đúng:** pane chi tiết mở bên phải, hiện đủ 4 trường của dòng đó. Gửi `NOTIFY orders, '{"id": 7, "ten": "Lan"}';`, chọn ô payload của dòng đó: JSON hiện thụt lề.
- [ ] Bôi chọn vài ô, nhấn **Ctrl+C**, dán vào Notepad hoặc Excel.
  **Đúng:** dán ra đúng các ô, cách nhau bằng tab; status bar ghi "Copied n cells".

### 1.4 Dừng, nghe lại, xóa

- [ ] Nhấn **Shift+F5** (hoặc bấm **Stop**).
  **Đúng:** status bar ghi **Stopped**; các sự kiện vẫn còn để xem; ô Channels gõ lại được; Listen sáng, Stop mờ.
- [ ] Ở tab query chạy `NOTIFY orders, 'sau khi dừng';`.
  **Đúng:** tab Live không thêm dòng.
- [ ] Ở tab query chạy `select count(*) from pg_stat_activity where application_name = 'Kemable' and query ilike 'listen%';` trước và sau khi Stop.
  **Đúng:** đang nghe thì là 1, sau Stop là 0: kết nối nghe đã đóng trên server.
- [ ] Sửa ô Channels thành `orders, jobs` rồi **Listen**.
  **Đúng:** danh sách cũ còn nguyên cho tới khi server nhận xong lệnh nghe, rồi được thay bằng danh sách mới trống; `NOTIFY jobs, 'a';` và `NOTIFY orders, 'b';` đều hiện.
- [ ] Stop rồi bấm **Clear** trên thanh công cụ.
  **Đúng:** về lại "Nothing heard yet"; nhãn tab hết số. Khi đang nghe thì nút Clear mờ.

### 1.5 Tên có chữ hoa, dấu cách

- [ ] Gõ `"Order Events"` (có nháy kép) vào Channels, Listen. Ở tab query chạy `select pg_notify('Order Events', 'đúng tên');` rồi `select pg_notify('order events', 'sai tên');`.
  **Đúng:** chỉ dòng `đúng tên` hiện ra, cột channel ghi `Order Events`.

### 1.6 Nhiều tab, đổi database, đóng

- [ ] Mở thêm một tab Live nữa (**Ctrl+Alt+L**), nghe `jobs`; tab đầu nghe `orders`. Gửi vào từng channel.
  **Đúng:** tab thứ hai tên **Live 2**; mỗi tab chỉ nhận channel của mình; chuyển qua lại thì mỗi tab giữ danh sách, vị trí cuộn và filter riêng.
- [ ] Đang nghe ở database `demo_sales`, chuyển database trên toolbar sang `postgres`, mở tab query và chạy `NOTIFY orders, 'khác database';`.
  **Đúng:** tab Live **không** nhận (`NOTIFY` chỉ tới người nghe cùng database); status bar của tab Live vẫn ghi `in demo_sales`, và tab hiện dải vàng "Listening in demo_sales, while queries now run in postgres". Đổi toolbar về `demo_sales` thì dải vàng mất. Stop rồi Listen lại thì nghe ở `postgres`.
- [ ] Đóng tab Live đang nghe bằng **Ctrl+W**, rồi chạy lại câu đếm `pg_stat_activity` ở 1.4.
  **Đúng:** tab đóng ngay, không hỏi; kết quả đếm là 0.
- [ ] Mở tab Live, Listen, rồi bấm **Disconnect** trong menu kết nối (hoặc mở một kết nối khác).
  **Đúng:** ngắt được ngay; không còn tab Live; không có thông báo lỗi nào hiện ra sau đó.

### 1.7 Lỗi và giới hạn

- [ ] Đang nghe, khởi động lại dịch vụ PostgreSQL (Services › postgresql › Restart).
  **Đúng:** tab Live hiện dải đỏ "The connection was lost" kèm lời của server; các sự kiện đã nhận còn nguyên; bấm Listen thì nghe lại được sau khi server lên.
- [ ] Listen với Channels là `orders`, rồi chạy `select count(pg_notify('orders', g::text)) from generate_series(1, 100001) g;`.
  **Đúng:** tab dừng ở đúng 100.000 sự kiện, hiện dải vàng "Stopped at 100,000 events", status bar ghi Stopped. Clear rồi Listen thì nghe tiếp được.
- [ ] (Nếu có Supabase) kết nối qua pooler cổng 6543, mở tab Live, Listen.
  **Đúng:** có dải thông báo rằng pooler ở chế độ transaction không giữ `LISTEN`, hãy kết nối trực tiếp cổng 5432.

---

## 2. Tab Live với Redis (pub/sub)

Cần một Redis như mục 5 của `docs/testing/phase-2.md`.

### 2.1 Channel và pattern

- [ ] Kết nối Redis, nhấn **Ctrl+Alt+L**.
  **Đúng:** tab Live có **hai** ô: **Channels** và **Patterns**; gợi ý ghi `PUBLISH news hello`.
- [ ] Gõ `news` vào Channels, `user:*` vào Patterns, nhấn Enter.
  **Đúng:** status bar ghi Listening; grid có 4 cột `received`, `channel`, `pattern`, `message`.
- [ ] Ở tab query chạy từng dòng:
  ```
  PUBLISH news "xin chào"
  PUBLISH user:7 login
  PUBLISH khac x
  ```
  **Đúng:** tab query báo số người nhận (1, 1, 0). Tab Live có 2 dòng: `news` với cột pattern là NULL và message `xin chào`; `user:7` với pattern `user:*` và message `login`. Dòng thứ ba không tới.
- [ ] Chỉ điền Patterns (để trống Channels), Listen.
  **Đúng:** nghe được bình thường.
- [ ] Trong console query gõ `SUBSCRIBE news` rồi chạy.
  **Đúng:** vẫn bị từ chối như trước ("not run from the console"): nghe là việc của tab Live.

### 2.2 Sự kiện của key (keyspace notifications)

- [ ] Ở tab query chạy `CONFIG SET notify-keyspace-events ""`. Ở tab Live gõ `__keyspace@0__:*` vào Patterns, Listen.
  **Đúng:** có dải thông báo rằng `notify-keyspace-events` đang trống nên server không gửi sự kiện key, kèm lệnh bật `CONFIG SET notify-keyspace-events KEA`.
- [ ] Chạy `CONFIG SET notify-keyspace-events KEA`, Stop rồi Listen lại, sau đó chạy `SET demo 1`, `EXPIRE demo 1`, đợi 2 giây.
  **Đúng:** lần này không có dải thông báo; tab Live nhận các dòng channel `__keyspace@0__:demo` với message `set`, `expire`, rồi `expired`.
- [ ] Thử xong chạy `CONFIG SET notify-keyspace-events ""` để trả server về như cũ.

### 2.3 Kết nối im lặng lâu, mất kết nối

- [ ] Listen một channel không ai gửi, để yên 3 phút, rồi `PUBLISH` vào nó.
  **Đúng:** vẫn nhận được; tab không tự chuyển sang Stopped (cứ 30 giây app gửi `PING` để giữ kết nối).
- [ ] Đang nghe, tắt Redis (dừng dịch vụ Memurai, hoặc `docker stop`).
  **Đúng:** trong vòng một phút tab hiện dải đỏ "The connection was lost"; sự kiện đã nhận còn nguyên.
- [ ] (Nếu server có ACL) đăng nhập bằng user không có quyền trên channel, Listen channel đó.
  **Đúng:** dải đỏ "Access denied" kèm lời `NOPERM` của server; grid không mở danh sách trống.

### 2.4 Mở key khi đang ở tab Live

- [ ] Đang ở tab Live, bấm một key trong pane **Keys** ở sidebar.
  **Đúng:** app chuyển sang tab query và mở key ở đó; tab Live vẫn nghe tiếp ở phía sau.

---

## 3. Giao diện chung của tab Live

- [ ] Đổi Light / Dark trong Settings khi tab Live đang mở.
  **Đúng:** hàng Channels, grid, các dải thông báo, empty state đều đổi màu đúng, không có chỗ nào chữ khó đọc.
- [ ] Bật **Compact rows** trong Settings.
  **Đúng:** grid của tab Live cũng dùng dòng 24 px.
- [ ] Chỉ dùng bàn phím: Ctrl+Alt+L, gõ channel, Enter, Tab tới các nút, Shift+F5.
  **Đúng:** làm được hết, không cần chuột; rê chuột lên mỗi nút thấy tip có ghi phím tắt.
- [ ] Thu hẹp cửa sổ khi tab Live đang mở.
  **Đúng:** hai ô Channels và Patterns xuống dòng thay vì tràn; các nút trên thanh tab không đè lên nhau.
- [ ] Tắt app khi tab Live đang nghe, mở lại.
  **Đúng:** app tắt bình thường; mở lại thì các tab query trở về, tab Live thì không (tab Live không được giữ qua các lần mở).

---

## 4. Dòng trống kết thúc một câu lệnh (PostgreSQL, SQLite)

Script viết không có dấu `;` vẫn chạy được từng câu, khi các câu cách nhau một dòng trống (`docs/decisions/0021-blank-line-ends-a-statement.md`). SQL Server không đổi: cả batch giữa hai dòng `GO` vẫn gửi nguyên.

- [ ] Kết nối `Local PG`, gõ vào editor (không có `;`, có một dòng trống ở giữa):
  ```sql
  select 1

  select * from pg_tables
  ```
  Đặt con trỏ ở dòng `select * from pg_tables`, nhấn **Ctrl+Enter**.
  **Đúng:** chỉ câu đó chạy, có kết quả; status bar ghi "Ran 1 of 2 statements"; editor tô khung đúng dòng đó. Không còn lỗi "syntax error".
- [ ] Đặt con trỏ ở dòng `select 1`, nhấn **Ctrl+Enter**.
  **Đúng:** chỉ `select 1` chạy, ra một dòng.
- [ ] Nhấn **Ctrl+Shift+Enter** (Run all).
  **Đúng:** hai tab kết quả, Result 1 và Result 2; status bar ghi "Ran 2 statements".
- [ ] Gõ một câu có dòng trống ở giữa, rồi Run all:
  ```sql
  with x as (select 1 as a)

  select a

  from x

  where a = 1
  ```
  **Đúng:** vẫn là **một** câu, chạy ra một dòng: dòng trống bên trong một câu không cắt nó.
- [ ] Gõ `update pg_tables` (đừng chạy), dòng trống, `set x = 1`, dòng trống, `select 2`, rồi xem status bar sau khi nhấn Ctrl+Enter ở dòng cuối.
  **Đúng:** "Ran 1 of 2 statements": `set` không mở câu mới (nó thuộc về `update`), còn `select 2` thì có.
- [ ] Kết nối `Local MSSQL`, gõ `declare @x int = 1`, dòng trống, `select @x`, nhấn Ctrl+Enter.
  **Đúng:** cả hai dòng chạy như một batch, ra `1`: SQL Server không tách theo dòng trống.

---

## 5. Sort trong Kemable với kết quả lớn

Thử trên bản release (`pnpm tauri build` rồi chạy app đã cài, hoặc `target/release/kemable-windows.exe`): bản `pnpm tauri dev` không tối ưu code nên chậm hơn nhiều lần, thời gian ở đó không nói lên điều gì.

- [ ] Mở **In memory** (SQLite), đặt Limit là **No limit**, chạy:
  ```sql
  with recursive c(x) as (select 1 union all select x + 1 from c limit 2000000)
  select x * 7919 % 2000003 as n, 'khách ' || (x * 7919 % 2000003) as ten from c
  ```
  **Đúng:** 2.000.000 dòng, các số ở cột `n` không theo thứ tự.
- [ ] Bấm **Filter**, lọc `n` **is at least** `0`, Apply. Bộ lọc này giữ mọi dòng; nó chỉ để buộc Kemable tự sort các dòng đang giữ thay vì chạy lại câu lệnh với `ORDER BY` (ADR 0005, ADR 0011).
  **Đúng:** status bar ghi `2,000,000 of 2,000,000 rows held`.
- [ ] Bấm tiêu đề cột `n`.
  **Đúng:** sort xong gần như tức thì (dưới nửa giây), status bar ghi "sorted in Kemable"; trong lúc sort grid vẫn cuộn được. Dòng đầu là `0`.
- [ ] Bấm tiêu đề cột `ten`.
  **Đúng:** xong trong khoảng nửa giây; `khách 2` đứng trước `khách 10`, vì số nằm trong chữ được so theo giá trị.
- [ ] Bấm lại cùng tiêu đề (giảm dần), rồi giữ **Shift** bấm thêm tiêu đề `n`.
  **Đúng:** đảo chiều đúng; sort theo hai cột cũng xong trong khoảng nửa giây, và trong mỗi nhóm `ten` bằng nhau thì `n` tăng dần.

---

## Test tự động chạy trên server thật

Hai test của phần nghe cần server thật (thay mật khẩu):

```bash
set KEMABLE_POSTGRES_URL=postgres://postgres:<mật khẩu>@localhost:5432/postgres?sslmode=disable
cargo test -p kemable-driver-postgres live -- --ignored
set KEMABLE_REDIS_URL=redis://localhost:6379
cargo test -p kemable-driver-redis listen -- --ignored
```

Mỗi test mở một người nghe, gửi một `NOTIFY` hoặc `PUBLISH` tới khi người nghe nhận được, rồi dừng.
