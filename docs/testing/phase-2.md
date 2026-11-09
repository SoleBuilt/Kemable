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

## 4. Gợi ý khi gõ (autocomplete)

Kết nối `Local PG`, chọn database `demo_sales`.

- [ ] Gõ `select * from ord`.
  **Đúng:** danh sách hiện `orders`, `order_items` (icon bảng) cùng các từ khóa; Enter hoặc Tab chèn tên.
- [ ] Gõ `select * from public.`.
  **Đúng:** ngay sau dấu chấm hiện các bảng và view của schema `public`.
- [ ] Gõ `select o. from orders o` rồi đặt con trỏ ngay sau `o.`, nhấn **Ctrl+Space**.
  **Đúng:** chỉ hiện các cột của `orders`, có kiểu dữ liệu ở bên phải.
- [ ] Gõ `select ` rồi **Ctrl+Space** trong câu `select  from orders o join customers c on c.id = o.customer_id`.
  **Đúng:** hiện cột của cả hai bảng; cột trùng tên (`id`) chỉ hiện một lần, ghi `o, c`; cuối danh sách có alias `o`, `c`.
- [ ] Gõ `insert into customers (`, rồi gõ chữ đầu của một cột.
  **Đúng:** hiện các cột của `customers`.
- [ ] Gõ trong chuỗi `'...'` hoặc sau `--`.
  **Đúng:** không hiện tên bảng/cột.
- [ ] Tạo bảng có tên viết hoa hoặc có dấu cách: `create table "Bang Thu" ("Ma So" int);`, chạy, rồi gõ `select * from Ba`.
  **Đúng:** bảng mới có trong gợi ý ngay (không cần kết nối lại); chọn nó thì chèn `"Bang Thu"` có ngoặc kép. Gõ `select * from "Ba` rồi chọn thì ngoặc được đóng đúng, không bị thừa.
- [ ] Trên **SQL Server**, gõ `select * from dbo.` và `select [`.
  **Đúng:** hiện bảng của `dbo`; tên cần quote được chèn trong `[...]`.
- [ ] Trên **SQL Server**, script có nhiều câu không có `;` và có dòng `GO`: gõ ở câu sau `GO`.
  **Đúng:** alias của batch trước `GO` không lẫn vào.
- [ ] Đổi database ở toolbar (Ctrl+K) rồi gõ `select * from `.
  **Đúng:** gợi ý là bảng của database mới.
- [ ] Gõ nhanh một câu dài.
  **Đúng:** editor không khựng; danh sách hiện ra trong khoảng một khung hình sau lần đầu (lần đầu với mỗi bảng có thể chậm hơn chút vì phải đọc catalog).

## 5. Redis: key, kiểu giá trị, TTL

Máy hiện chưa có Redis. Cách có một server để thử, chọn một:

- **Memurai Developer** (Redis cho Windows, miễn phí cho dev): cài xong chạy ở `localhost:6379`.
- **Upstash** (Redis trên cloud, gói free): lấy URL `rediss://default:<mật khẩu>@<tên>.upstash.io:6379`.
- Docker hoặc WSL nếu sau này cài: `docker run -p 6379:6379 redis:7`.

Khi đã có server, chạy luôn test live: `set PABOABLE_REDIS_URL=redis://localhost:6379` rồi `cargo test -p paboable-driver-redis -- --ignored`.

Dữ liệu thử: kết nối xong, dán vào editor và **Run all** (F5):

```
SET greeting "xin chào"
SET session:1 abc EX 3600
HSET user:1 name "Nguyễn Văn A" email a@example.com age 30
RPUSH queue:jobs job1 job2 job3
SADD tags redis sql rust
ZADD leaderboard 100 an 250 binh 175 chi
XADD events * type login user 1
XADD events * type logout user 1
DEBUG POPULATE 5000 bulk:
```

Redis 7 từ chối `DEBUG` nếu chưa bật `enable-debug-command`; khi đó bỏ dòng cuối, phần "Load more" thử bằng Limit và `COUNT` nhỏ hơn hoặc bỏ qua.

- [ ] Bấm **Redis…**, Host `localhost`, Name `Local Redis`, **Connect**. Thử thêm: dán URL `redis://:matkhau@localhost:6379/0` vào ô URL; với Upstash dán URL `rediss://...`.
  **Đúng:** toolbar ghi "Redis 7.x"; sidebar có tab **Keys**; ô chọn database (Ctrl+K) có `0` đến `15`. Sai mật khẩu thì báo "Access denied" kèm `WRONGPASS`.
- [ ] Chạy dữ liệu thử ở trên.
  **Đúng:** mỗi dòng là một lệnh; `SET` ghi "Done" trong Messages; `HSET`, `RPUSH`... trả về một dòng số. Tab Keys tự quét lại.
- [ ] Ở tab **Keys**: gõ `user` rồi Enter; gõ `bulk:*` rồi Enter; chọn loại **Hash**.
  **Đúng:** chữ thường tìm ở bất kỳ đâu trong tên, `*` là glob; lọc loại chỉ còn hash. Có nhãn STR/HASH/LIST/SET/ZSET; `session:1` có TTL (ví dụ `60 min`). Với 5.000 key `bulk:*` có nút **Load more**.
- [ ] Bấm từng key: `greeting`, `user:1`, `queue:jobs`, `tags`, `leaderboard`, `events`.
  **Đúng:** thanh trên kết quả ghi tên key, loại, độ dài (bytes/fields/items/members/entries), "no expiry" hoặc "expires in ..."; grid: string là 1 dòng `value`; hash `field, value`; list `index, value`; set `member`; sorted set `member, score` (số); stream `id, fields` (JSON).
- [ ] Bấm tiêu đề cột `score` của `leaderboard`.
  **Đúng:** sort trong app (status bar ghi "sorted in Paboable").
- [ ] Tạo một list lớn: `DEBUG POPULATE` không tạo list, nên chạy trong editor một vòng `RPUSH big 1 2 3 ...` vài lần, hoặc dùng Limit nhỏ (1.000) rồi mở một hash/list lớn hơn 1.000 phần tử.
  **Đúng:** dừng ở row cap, có **Fetch all**; bấm thì đọc hết.
- [ ] Chạy `SELECT 1` rồi `SET only_in_1 x` trong cùng một script; sau đó chọn lại database `0` ở toolbar.
  **Đúng:** Messages ghi "Switched to 1"; toolbar đổi sang `1`; key `only_in_1` chỉ thấy ở database 1.
- [ ] Chạy `SUBSCRIBE news`, `MONITOR`, `AUTH x`.
  **Đúng:** báo lệnh không chạy từ console ("not run from the console"); kết nối vẫn dùng được.
- [ ] Chạy `BLPOP empty 0` rồi bấm **Stop**.
  **Đúng:** dừng ngay, Messages báo đã hủy; chạy `PING` sau đó vẫn ra `PONG`.
- [ ] Chạy `GET user:1`.
  **Đúng:** lỗi `WRONGTYPE` của server ở Messages.
- [ ] Chạy `CONFIG SET requirepass x` (trên server thử, rồi đặt lại) và xem tab **History**.
  **Đúng:** câu đó trong History là `-- not kept: this statement holds a password`.
- [ ] Đợi một key có TTL hết hạn (`SET temp 1 EX 5`), rồi bấm vào nó trong danh sách.
  **Đúng:** status bar báo "Not read: ..."; danh sách tự quét lại và key đó biến mất.

## 6. Cloudflare D1 qua API

Cần một tài khoản Cloudflare (gói free đủ dùng) và một database D1:

1. Dashboard Cloudflare → **Storage & Databases → D1** → **Create**, đặt tên ví dụ `paboable-test`.
2. **Account ID**: ở trang tổng quan của tài khoản (cột phải), hoặc trong URL dashboard (`dash.cloudflare.com/<account id>/...`).
3. **API token**: My Profile → API Tokens → **Create Token** → Custom token, quyền **Account · D1 · Edit**.
4. Chạy luôn test live (thay giá trị):
   ```bash
   set PABOABLE_D1_ACCOUNT=<account id>
   set PABOABLE_D1_TOKEN=<token>
   set PABOABLE_D1_DATABASE=paboable-test
   cargo test -p paboable-driver-d1 -- --ignored
   ```

- [ ] Bấm **D1…**, nhập Account ID, API token, để trống Database, Name `D1 test`, giữ tick lưu token, **Connect**.
  **Đúng:** sidebar liệt kê các database D1 của tài khoản; chạy một câu khi chưa chọn database thì báo cần chọn database (Ctrl+K). Token sai báo "Access denied" kèm mã `10000`.
- [ ] Chọn `paboable-test` (Ctrl+K hoặc ở cây), chạy:
  ```sql
  create table items (id integer primary key, name text, price real, photo blob);
  insert into items (name, price, photo) values ('xin chào', 1.5, x'00ff'), ('b', 2, null);
  select * from items;
  ```
  **Đúng:** `create`/`insert` ghi "Done"/"2 rows affected"; Messages có dòng "D1: N rows read, M written, x ms" cho mỗi câu; `select` ra 2 dòng, `photo` hiện `0x00ff`, tiếng Việt đúng.
- [ ] Gõ sai `selec * from items`.
  **Đúng:** Messages báo lỗi của SQLite kèm mã `7500`, editor đánh dấu đúng vị trí lỗi.
- [ ] Double-click bảng `items` ở cây; thử phân trang, filter, sort (mục 3).
  **Đúng:** như với SQLite; câu Copy SQL dùng `LIMIT ... OFFSET ...`.
- [ ] Gõ `select * from it` và `select i. from items i` (Ctrl+Space sau dấu chấm).
  **Đúng:** gợi ý bảng `items` và các cột của nó (mục 4).
- [ ] Chạy một câu chậm (ví dụ `with recursive c(x) as (select 1 union all select x+1 from c limit 5000000) select count(*) from c;`) rồi bấm **Stop**.
  **Đúng:** app dừng chờ ngay và báo đã hủy (phía Cloudflare câu có thể vẫn chạy xong).
- [ ] Đóng app, mở lại, mở `D1 test` từ danh sách đã lưu.
  **Đúng:** kết nối mà không hỏi token (token ở Windows Credential Manager). Bỏ tick lưu token rồi mở lại thì app hỏi "API token".
- [ ] Nếu máy đi Internet qua proxy công ty: kết nối vẫn được, vì app dùng proxy của Windows.
