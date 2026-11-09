-- Demo data for trying Paboable on PostgreSQL: a small Vietnamese online shop.
--
--   10 categories, 70 products, 150 customers, 3,000 orders, 10,000 order lines, 8,000 payments.
--
-- Run it on an empty database, or one whose six tables exist and are empty, in one
-- transaction, with UTF-8 as the client encoding:
--
--   createdb demo_sales
--   PGCLIENTENCODING=UTF8 psql -d demo_sales -v ON_ERROR_STOP=1 -1 -f scripts/demo/postgres-demo-sales.sql
--
-- The seed is fixed, so every run gives the same rows. "Now" is fixed too
-- (2026-11-09 12:00 +07), so order statuses and due payments do not drift.
--
-- Shape of the data:
--   - Customers order unevenly: a few order often, many order a few times, some never.
--   - Orders hold 1 to many distinct products; cheap products sell more often and in larger quantities.
--   - order_items.unit_price is the price paid, sometimes discounted from the list price.
--   - orders.total_amount is the lines' sum plus 30,000 shipping under 500,000.
--   - Payments: many orders are paid once (COD or online); others in installments a month apart,
--     some after a failed first attempt. Completed and pending installments sum to the order total.
--     Installments due after "now" are Pending. Cancelled orders have one Refunded or Failed payment.

-- 1. Bảng Khách hàng
CREATE TABLE IF NOT EXISTS customers (
    customer_id SERIAL PRIMARY KEY,
    full_name VARCHAR(150) NOT NULL,
    email VARCHAR(150) UNIQUE NOT NULL,
    phone_number VARCHAR(20),
    address TEXT,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- 2. Bảng Danh mục sản phẩm
CREATE TABLE IF NOT EXISTS categories (
    category_id SERIAL PRIMARY KEY,
    category_name VARCHAR(100) NOT NULL,
    description TEXT
);

-- 3. Bảng Sản phẩm
CREATE TABLE IF NOT EXISTS products (
    product_id SERIAL PRIMARY KEY,
    category_name VARCHAR(100), -- hoặc dùng category_id REFERENCES categories(category_id)
    category_id INT REFERENCES categories(category_id) ON DELETE SET NULL,
    product_name VARCHAR(200) NOT NULL,
    description TEXT,
    price DECIMAL(12, 2) NOT NULL CHECK (price >= 0),
    stock_quantity INT NOT NULL DEFAULT 0 CHECK (stock_quantity >= 0),
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- 4. Bảng Đơn hàng
CREATE TABLE IF NOT EXISTS orders (
    order_id SERIAL PRIMARY KEY,
    customer_id INT REFERENCES customers(customer_id) ON DELETE CASCADE,
    order_date TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    total_amount DECIMAL(12, 2) NOT NULL CHECK (total_amount >= 0),
    status VARCHAR(50) DEFAULT 'Pending', -- Pending, Processing, Shipped, Delivered, Cancelled
    shipping_address TEXT NOT NULL
);

-- 5. Bảng Chi tiết đơn hàng (Lưu danh sách sản phẩm trong mỗi đơn)
CREATE TABLE IF NOT EXISTS order_items (
    order_item_id SERIAL PRIMARY KEY,
    order_id INT REFERENCES orders(order_id) ON DELETE CASCADE,
    product_id INT REFERENCES products(product_id) ON DELETE SET NULL,
    quantity INT NOT NULL CHECK (quantity > 0),
    unit_price DECIMAL(12, 2) NOT NULL CHECK (unit_price >= 0) -- Giá tại thời điểm mua
);

-- 6. Bảng Thanh toán
CREATE TABLE IF NOT EXISTS payments (
    payment_id SERIAL PRIMARY KEY,
    order_id INT REFERENCES orders(order_id) ON DELETE CASCADE,
    payment_date TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    amount DECIMAL(12, 2) NOT NULL CHECK (amount >= 0),
    payment_method VARCHAR(50) NOT NULL, -- COD, Credit Card, Bank Transfer, MoMo...
    payment_status VARCHAR(50) DEFAULT 'Pending' -- Pending, Completed, Failed, Refunded
);

-- The tables may already exist, created by hand from the statements above.
-- Load only into empty tables, and start their ids at 1, which the data relies on.
DO $$
DECLARE
    name text;
    has_rows boolean;
BEGIN
    FOREACH name IN ARRAY ARRAY['customers', 'categories', 'products', 'orders', 'order_items', 'payments'] LOOP
        EXECUTE format('SELECT EXISTS (SELECT 1 FROM %I)', name) INTO has_rows;
        IF has_rows THEN
            RAISE EXCEPTION 'table % already holds rows; load the demo data into empty tables', name;
        END IF;
    END LOOP;
END $$;
TRUNCATE customers, categories, products, orders, order_items, payments RESTART IDENTITY;

-- ---------------------------------------------------------------------------
-- Helpers, dropped with the session
-- ---------------------------------------------------------------------------

SELECT setseed(0.20261109);

CREATE FUNCTION pg_temp.pick(items text[], r float8) RETURNS text
LANGUAGE sql IMMUTABLE AS $$ SELECT items[1 + floor(r * array_length(items, 1))::int] $$;

-- A street address, from four random numbers.
CREATE FUNCTION pg_temp.address(r_city float8, r_district float8, r_street float8, r_house float8) RETURNS text
LANGUAGE sql IMMUTABLE AS $$
    SELECT CASE WHEN r_house > 0.7
                THEN (1 + floor(r_house * 400))::int || '/' || (1 + floor(r_street * 40))::int
                ELSE (1 + floor(r_house * 400))::int::text END
        || ' ' || pg_temp.pick(ARRAY['Nguyễn Huệ', 'Lê Lợi', 'Trần Hưng Đạo', 'Hai Bà Trưng', 'Lý Thường Kiệt',
                                     'Nguyễn Trãi', 'Điện Biên Phủ', 'Cách Mạng Tháng 8', 'Võ Văn Tần', 'Pasteur',
                                     'Nguyễn Văn Linh', 'Lê Duẩn', 'Phan Đình Phùng', 'Trường Chinh',
                                     'Hoàng Văn Thụ', 'Quang Trung'], r_street)
        || ', ' || CASE
            WHEN r_city < 0.45 THEN pg_temp.pick(ARRAY['Quận 1', 'Quận 3', 'Quận 5', 'Quận 7', 'Quận 10',
                 'Quận Bình Thạnh', 'Quận Phú Nhuận', 'Quận Tân Bình', 'Quận Gò Vấp', 'TP. Thủ Đức'], r_district)
                 || ', TP. Hồ Chí Minh'
            WHEN r_city < 0.75 THEN pg_temp.pick(ARRAY['Quận Ba Đình', 'Quận Hoàn Kiếm', 'Quận Đống Đa',
                 'Quận Cầu Giấy', 'Quận Hai Bà Trưng', 'Quận Thanh Xuân', 'Quận Tây Hồ', 'Quận Long Biên'], r_district)
                 || ', Hà Nội'
            WHEN r_city < 0.83 THEN pg_temp.pick(ARRAY['Quận Hải Châu', 'Quận Thanh Khê', 'Quận Sơn Trà',
                 'Quận Ngũ Hành Sơn'], r_district) || ', Đà Nẵng'
            WHEN r_city < 0.88 THEN pg_temp.pick(ARRAY['Quận Ninh Kiều', 'Quận Cái Răng'], r_district) || ', Cần Thơ'
            WHEN r_city < 0.93 THEN pg_temp.pick(ARRAY['Quận Lê Chân', 'Quận Ngô Quyền', 'Quận Hồng Bàng'], r_district)
                 || ', Hải Phòng'
            WHEN r_city < 0.96 THEN pg_temp.pick(ARRAY['Phường Tân Phong', 'Phường Trảng Dài'], r_district)
                 || ', Biên Hòa, Đồng Nai'
            ELSE pg_temp.pick(ARRAY['Phường Lộc Thọ', 'Phường Vĩnh Hòa'], r_district) || ', Nha Trang, Khánh Hòa'
        END
$$;

-- ---------------------------------------------------------------------------
-- Categories and products
-- ---------------------------------------------------------------------------

INSERT INTO categories (category_name, description) VALUES
    ('Điện thoại & Máy tính bảng', 'Điện thoại thông minh và máy tính bảng chính hãng'),
    ('Laptop & Máy tính', 'Laptop văn phòng, đồ họa và gaming'),
    ('Phụ kiện điện tử', 'Tai nghe, sạc, cáp, chuột, bàn phím và phụ kiện khác'),
    ('Thiết bị gia dụng', 'Đồ điện gia dụng cho nhà bếp và gia đình'),
    ('Thời trang nam', 'Quần áo, giày dép và phụ kiện nam'),
    ('Thời trang nữ', 'Quần áo, túi xách và giày dép nữ'),
    ('Mỹ phẩm & Làm đẹp', 'Chăm sóc da, trang điểm và chăm sóc tóc'),
    ('Mẹ & Bé', 'Sữa, tã và đồ dùng cho mẹ và bé'),
    ('Sách & Văn phòng phẩm', 'Sách tiếng Việt và dụng cụ văn phòng'),
    ('Thể thao & Dã ngoại', 'Dụng cụ tập luyện, giày thể thao và đồ cắm trại');

CREATE TEMP TABLE seed_products (ord serial, category_id int, product_name text, price numeric(12, 2));
INSERT INTO seed_products (category_id, product_name, price) VALUES
    (1, 'iPhone 15 128GB', 19990000), (1, 'iPhone 15 Pro Max 256GB', 31990000),
    (1, 'Samsung Galaxy S24 Ultra 256GB', 28990000), (1, 'Samsung Galaxy A55 5G', 9490000),
    (1, 'Xiaomi Redmi Note 13 Pro', 6990000), (1, 'OPPO Reno11 F 5G', 8990000),
    (1, 'iPad Air M2 11 inch WiFi', 16990000), (1, 'Samsung Galaxy Tab S9 FE', 9990000),
    (1, 'Xiaomi Pad 6', 7490000),
    (2, 'MacBook Air M3 13 inch 8GB/256GB', 27990000), (2, 'Dell Inspiron 15 3530 i5', 15490000),
    (2, 'ASUS Vivobook 15 OLED', 14990000), (2, 'Lenovo ThinkPad E14 Gen 5', 19990000),
    (2, 'HP Pavilion 14', 16490000), (2, 'Acer Nitro V 15 RTX 4050', 23990000),
    (3, 'Tai nghe AirPods Pro 2', 5990000), (3, 'Tai nghe Sony WH-1000XM5', 7490000),
    (3, 'Sạc nhanh Anker 65W GaN', 890000), (3, 'Cáp USB-C to USB-C Baseus 1m', 149000),
    (3, 'Pin sạc dự phòng Xiaomi 20000mAh', 499000), (3, 'Chuột Logitech MX Master 3S', 2490000),
    (3, 'Bàn phím cơ Keychron K2', 1990000), (3, 'Ốp lưng iPhone 15 MagSafe', 290000),
    (3, 'Loa Bluetooth JBL Flip 6', 2690000), (3, 'Thẻ nhớ SanDisk 128GB', 259000),
    (4, 'Nồi cơm điện Cuckoo 1.8L', 2190000), (4, 'Nồi chiên không dầu Philips 4.1L', 2590000),
    (4, 'Máy lọc không khí Xiaomi 4 Lite', 2990000), (4, 'Robot hút bụi Ecovacs T20 Omni', 12990000),
    (4, 'Quạt điều hòa Kangaroo', 3290000), (4, 'Máy xay sinh tố Sunhouse', 690000),
    (4, 'Bình đun siêu tốc Lock&Lock 1.7L', 390000), (4, 'Bàn ủi hơi nước Philips', 990000),
    (5, 'Áo thun nam cotton basic', 159000), (5, 'Áo sơ mi nam dài tay Oxford', 349000),
    (5, 'Quần jean nam slim fit', 459000), (5, 'Quần kaki nam', 399000),
    (5, 'Áo khoác gió nam chống nước', 529000), (5, 'Giày sneaker nam da', 890000),
    (5, 'Thắt lưng da nam', 299000),
    (6, 'Đầm suông công sở', 429000), (6, 'Áo kiểu nữ cổ nơ', 259000), (6, 'Chân váy chữ A', 289000),
    (6, 'Quần jean nữ ống rộng', 449000), (6, 'Áo len nữ dệt kim', 379000),
    (6, 'Túi xách nữ đeo chéo', 599000), (6, 'Giày cao gót mũi nhọn 7cm', 499000),
    (6, 'Áo dài cách tân', 690000),
    (7, 'Kem chống nắng La Roche-Posay Anthelios 50ml', 495000), (7, 'Sữa rửa mặt CeraVe 236ml', 389000),
    (7, 'Nước tẩy trang Bioderma 500ml', 459000), (7, 'Serum Vitamin C Klairs 35ml', 359000),
    (7, 'Son kem lì Black Rouge', 189000), (7, 'Kem dưỡng ẩm Cetaphil 453g', 429000),
    (7, 'Dầu gội Tsubaki 490ml', 239000),
    (8, 'Sữa bột Meiji 800g', 549000), (8, 'Tã quần Bobby size L 68 miếng', 359000),
    (8, 'Bình sữa Pigeon 240ml', 289000), (8, 'Xe đẩy em bé gấp gọn', 2490000),
    (8, 'Khăn ướt Mamamy 100 tờ (lốc 6)', 199000),
    (9, 'Sách Đắc Nhân Tâm', 86000), (9, 'Sách Nhà Giả Kim', 79000),
    (9, 'Sách Sapiens: Lược sử loài người', 189000), (9, 'Bút bi Thiên Long (hộp 20 cây)', 70000),
    (9, 'Sổ tay bìa da A5', 99000),
    (10, 'Thảm tập yoga TPE 6mm', 259000), (10, 'Tạ tay 5kg (cặp)', 390000),
    (10, 'Giày chạy bộ Nike Revolution 7', 1690000), (10, 'Lều cắm trại 4 người', 1290000),
    (10, 'Bình giữ nhiệt Lock&Lock 500ml', 329000);

INSERT INTO products (category_name, category_id, product_name, description, price, stock_quantity, created_at)
SELECT c.category_name, s.category_id, s.product_name,
       CASE WHEN s.category_id IN (1, 2, 3, 4) THEN 'Hàng chính hãng, bảo hành 12 tháng'
            WHEN s.category_id IN (5, 6) THEN 'Đổi size miễn phí trong 7 ngày'
            ELSE 'Hàng chính hãng, giao nhanh toàn quốc' END,
       s.price,
       CASE WHEN s.price > 10000000 THEN 3 + floor(random() * 40) ELSE 20 + floor(random() * 480) END,
       timestamptz '2024-01-02 09:00+07' + random() * interval '200 days'
FROM seed_products s JOIN categories c USING (category_id)
ORDER BY s.ord;

-- ---------------------------------------------------------------------------
-- Customers
-- ---------------------------------------------------------------------------

CREATE TEMP TABLE pick_customers AS
SELECT id, random() AS r_last, random() AS r_gender, random() AS r_mid, random() AS r_first,
       random() AS r_city, random() AS r_district, random() AS r_street, random() AS r_house,
       random() AS r_prefix, floor(random() * 10000000)::int AS digits, random() AS r_created, random() AS r_mail
FROM generate_series(1, 150) AS id;

INSERT INTO customers (full_name, email, phone_number, address, created_at)
SELECT n.last_vn || ' ' || n.middle || ' ' || n.first_vn,
       n.first_ascii || '.' || n.last_ascii || p.id || '@'
           || pg_temp.pick(ARRAY['gmail.com', 'gmail.com', 'gmail.com', 'gmail.com', 'gmail.com', 'gmail.com',
                                 'yahoo.com', 'outlook.com', 'icloud.com', 'hotmail.com'], p.r_mail),
       pg_temp.pick(ARRAY['090', '091', '093', '094', '096', '097', '098', '086', '032', '033', '035', '037',
                          '039', '070', '077', '079', '081', '083', '085', '088'], p.r_prefix)
           || lpad(p.digits::text, 7, '0'),
       pg_temp.address(p.r_city, p.r_district, p.r_street, p.r_house),
       timestamptz '2024-01-05 08:00+07'
           + p.r_created * (timestamptz '2026-10-31 20:00+07' - timestamptz '2024-01-05 08:00+07')
FROM pick_customers p
CROSS JOIN LATERAL (
    SELECT (ARRAY['Nguyễn', 'Nguyễn', 'Nguyễn', 'Nguyễn', 'Nguyễn', 'Nguyễn', 'Nguyễn', 'Trần', 'Trần', 'Trần',
                  'Lê', 'Lê', 'Lê', 'Phạm', 'Phạm', 'Hoàng', 'Huỳnh', 'Phan', 'Vũ', 'Võ', 'Đặng', 'Bùi', 'Đỗ',
                  'Hồ', 'Ngô', 'Dương', 'Lý'])[li] AS last_vn,
           (ARRAY['nguyen', 'nguyen', 'nguyen', 'nguyen', 'nguyen', 'nguyen', 'nguyen', 'tran', 'tran', 'tran',
                  'le', 'le', 'le', 'pham', 'pham', 'hoang', 'huynh', 'phan', 'vu', 'vo', 'dang', 'bui', 'do',
                  'ho', 'ngo', 'duong', 'ly'])[li] AS last_ascii,
           CASE WHEN p.r_gender < 0.5
                THEN pg_temp.pick(ARRAY['Văn', 'Hữu', 'Minh', 'Đức', 'Quang', 'Gia', 'Thành', 'Hoàng'], p.r_mid)
                ELSE pg_temp.pick(ARRAY['Thị', 'Thị', 'Ngọc', 'Thu', 'Thanh', 'Hoài', 'Bảo', 'Khánh'], p.r_mid)
           END AS middle,
           CASE WHEN p.r_gender < 0.5
                THEN (ARRAY['Anh', 'Bảo', 'Cường', 'Dũng', 'Duy', 'Đạt', 'Hải', 'Hiếu', 'Hoàng', 'Huy', 'Khang',
                            'Khoa', 'Long', 'Minh', 'Nam', 'Phúc', 'Quân', 'Sơn', 'Thắng', 'Tuấn', 'Việt', 'Vinh'])[fi]
                ELSE (ARRAY['An', 'Anh', 'Chi', 'Diệp', 'Giang', 'Hà', 'Hạnh', 'Hằng', 'Hoa', 'Hương', 'Lan',
                            'Linh', 'Mai', 'My', 'Ngân', 'Nhung', 'Phương', 'Quỳnh', 'Thảo', 'Trang', 'Uyên', 'Vy'])[fi]
           END AS first_vn,
           CASE WHEN p.r_gender < 0.5
                THEN (ARRAY['anh', 'bao', 'cuong', 'dung', 'duy', 'dat', 'hai', 'hieu', 'hoang', 'huy', 'khang',
                            'khoa', 'long', 'minh', 'nam', 'phuc', 'quan', 'son', 'thang', 'tuan', 'viet', 'vinh'])[fi]
                ELSE (ARRAY['an', 'anh', 'chi', 'diep', 'giang', 'ha', 'hanh', 'hang', 'hoa', 'huong', 'lan',
                            'linh', 'mai', 'my', 'ngan', 'nhung', 'phuong', 'quynh', 'thao', 'trang', 'uyen', 'vy'])[fi]
           END AS first_ascii
    FROM (SELECT 1 + floor(p.r_last * 27)::int AS li, 1 + floor(p.r_first * 22)::int AS fi) AS idx
) AS n
ORDER BY p.r_created;

-- ---------------------------------------------------------------------------
-- Orders: each customer has a buying appetite (log-normal) times the days
-- they have been a customer, so old, keen customers order often, new ones a
-- few times or not yet. Dates lean towards recent months, as a growing shop's do.
-- ---------------------------------------------------------------------------

CREATE TEMP TABLE customer_weights AS
SELECT customer_id,
       sum(w) OVER (ORDER BY customer_id) - w AS lo,
       sum(w) OVER (ORDER BY customer_id) AS hi,
       sum(w) OVER () AS total
FROM (
    SELECT customer_id,
           exp(0.75 * sqrt(-2 * ln(1 - random())) * cos(2 * pi() * random()))
               * greatest(extract(epoch FROM timestamptz '2026-11-09 12:00+07' - created_at) / 86400, 7) AS w
    FROM customers
) AS appetite;

CREATE TEMP TABLE pick_orders AS
SELECT g AS n, random() AS r_customer, random() AS r_date, random() AS r_status, random() AS r_address,
       random() AS a1, random() AS a2, random() AS a3, random() AS a4
FROM generate_series(1, 3000) AS g;

INSERT INTO orders (customer_id, order_date, total_amount, status, shipping_address)
SELECT c.customer_id, d.order_date, 0,
       CASE
           WHEN d.age < interval '2 days' THEN CASE WHEN p.r_status < 0.6 THEN 'Pending' ELSE 'Processing' END
           WHEN d.age < interval '7 days' THEN CASE WHEN p.r_status < 0.15 THEN 'Processing'
                                                    WHEN p.r_status < 0.85 THEN 'Shipped'
                                                    WHEN p.r_status < 0.93 THEN 'Delivered'
                                                    ELSE 'Cancelled' END
           ELSE CASE WHEN p.r_status < 0.9 THEN 'Delivered' ELSE 'Cancelled' END
       END,
       CASE WHEN p.r_address < 0.85 THEN c.address ELSE pg_temp.address(p.a1, p.a2, p.a3, p.a4) END
FROM pick_orders p
JOIN customer_weights cw ON p.r_customer * cw.total >= cw.lo AND p.r_customer * cw.total < cw.hi
JOIN customers c ON c.customer_id = cw.customer_id
CROSS JOIN LATERAL (
    SELECT c.created_at + (timestamptz '2026-11-09 12:00+07' - c.created_at) * power(p.r_date, 0.6) AS order_date
) AS o
CROSS JOIN LATERAL (SELECT o.order_date, timestamptz '2026-11-09 12:00+07' - o.order_date AS age) AS d
ORDER BY d.order_date;

-- ---------------------------------------------------------------------------
-- Order lines: lines per order follow a geometric draw (one-line orders are
-- the most common, big orders are rare), then single lines are added or
-- removed at random orders until there are exactly 10,000. Products are drawn
-- without repeats in an order, weighted to cheap ones.
-- ---------------------------------------------------------------------------

CREATE TEMP TABLE item_counts AS
SELECT order_id, 1 + floor(ln(1 - random()) / ln(0.7))::int AS n FROM orders;

WITH gap AS (SELECT 10000 - sum(n) AS d FROM item_counts),
     chosen AS (
         SELECT order_id FROM item_counts, gap
         WHERE (gap.d > 0 AND n < 20) OR (gap.d < 0 AND n > 1)
         ORDER BY random()
         LIMIT (SELECT abs(d) FROM gap)
     )
UPDATE item_counts
SET n = n + CASE WHEN (SELECT d FROM gap) > 0 THEN 1 ELSE -1 END
WHERE order_id IN (SELECT order_id FROM chosen);

CREATE TEMP TABLE product_weights AS
SELECT product_id, price, (0.5 + random()) / sqrt(price / 100000.0) AS weight FROM products;

INSERT INTO order_items (order_id, product_id, quantity, unit_price)
SELECT order_id, product_id,
       CASE WHEN price < 300000 THEN 1 + floor(power(random(), 2.5) * 6)::int
            ELSE 1 + floor(power(random(), 6) * 3)::int END,
       CASE WHEN random() < 0.2 THEN round(price * (0.8 + random() * 0.15) / 1000) * 1000 ELSE price END
FROM (
    SELECT ic.order_id, ic.n, pw.product_id, pw.price,
           row_number() OVER (PARTITION BY ic.order_id ORDER BY -ln(1 - random()) / pw.weight) AS rn
    FROM item_counts ic CROSS JOIN product_weights pw
) AS ranked
WHERE rn <= n
ORDER BY order_id, rn;

UPDATE orders o
SET total_amount = s.subtotal + CASE WHEN s.subtotal < 500000 THEN 30000 ELSE 0 END
FROM (SELECT order_id, sum(quantity * unit_price) AS subtotal FROM order_items GROUP BY order_id) AS s
WHERE s.order_id = o.order_id;

-- ---------------------------------------------------------------------------
-- Payments: every order gets one; "installment" orders (likelier when large,
-- never when cancelled) share the 5,000 others, so the total is exactly 8,000.
-- ---------------------------------------------------------------------------

CREATE TEMP TABLE pay_plan AS
SELECT order_id, status, order_date, total_amount,
       status <> 'Cancelled' AND random() < CASE WHEN total_amount >= 5000000 THEN 0.95
                                                 WHEN total_amount >= 1000000 THEN 0.7
                                                 ELSE 0.35 END AS multi,
       random() AS r_method, random() AS r_fail, random() AS r_cancel, random() AS r_days,
       1 AS n
FROM orders;

CREATE TEMP TABLE multi_orders AS
SELECT order_id, row_number() OVER (ORDER BY order_id) AS idx FROM pay_plan WHERE multi;

UPDATE pay_plan p
SET n = 2 + coalesce(e.extra, 0)
FROM multi_orders m
LEFT JOIN (
    SELECT idx, count(*) AS extra
    FROM (SELECT 1 + floor(random() * (SELECT count(*) FROM multi_orders))::int AS idx
          FROM generate_series(1, 5000 - (SELECT count(*) FROM multi_orders)::int)) AS x
    GROUP BY idx
) AS e USING (idx)
WHERE p.order_id = m.order_id;

CREATE TEMP TABLE pay_rows AS
SELECT p.*, k,
       p.multi AND p.r_fail < 0.15 AS has_fail,
       0.5 + random() AS w,
       random() AS r_minutes
FROM pay_plan p CROSS JOIN LATERAL generate_series(1, p.n) AS k;

CREATE TEMP TABLE pay_amounts AS
WITH counted AS (
    SELECT *, NOT (has_fail AND k = 1) AS is_counted,
           k - has_fail::int AS j, n - has_fail::int AS cnt
    FROM pay_rows
), shares AS (
    SELECT *,
           CASE WHEN NOT is_counted THEN NULL
                WHEN NOT multi THEN total_amount
                WHEN j < cnt THEN floor(total_amount * w
                                        / sum(CASE WHEN is_counted THEN w END) OVER (PARTITION BY order_id)
                                        / 1000) * 1000
           END AS share
    FROM counted
), finished AS (
    SELECT *,
           CASE WHEN is_counted AND multi AND j = cnt
                THEN total_amount - coalesce(sum(share) OVER (PARTITION BY order_id), 0)
                ELSE share END AS amount_counted
    FROM shares
)
SELECT f.*,
       coalesce(f.amount_counted, lead(f.amount_counted) OVER (PARTITION BY f.order_id ORDER BY f.k)) AS amount
FROM finished f;

INSERT INTO payments (order_id, payment_date, amount, payment_method, payment_status)
SELECT order_id, payment_date, amount, method,
       CASE
           WHEN status = 'Cancelled' THEN CASE WHEN r_cancel < 0.6 THEN 'Refunded' ELSE 'Failed' END
           WHEN NOT is_counted THEN 'Failed'
           WHEN status = 'Pending' THEN 'Pending'
           WHEN payment_date > timestamptz '2026-11-09 12:00+07' THEN 'Pending'
           WHEN method = 'COD' AND status <> 'Delivered' THEN 'Pending'
           ELSE 'Completed'
       END
FROM (
    SELECT a.*,
           CASE WHEN a.multi THEN CASE WHEN a.r_method < 0.5 THEN 'Credit Card' WHEN a.r_method < 0.8 THEN 'Bank Transfer'
                                       WHEN a.r_method < 0.9 THEN 'MoMo' ELSE 'VNPay' END
                ELSE CASE WHEN a.r_method < 0.4 THEN 'COD' WHEN a.r_method < 0.6 THEN 'MoMo'
                          WHEN a.r_method < 0.8 THEN 'Bank Transfer' WHEN a.r_method < 0.9 THEN 'Credit Card'
                          WHEN a.r_method < 0.95 THEN 'ZaloPay' ELSE 'VNPay' END
           END AS method,
           CASE
               WHEN a.status = 'Cancelled' THEN a.order_date + (1 + floor(a.r_days * 3)) * interval '1 day'
               WHEN NOT a.multi AND a.r_method < 0.4
                   THEN a.order_date + (2 + floor(a.r_days * 4)) * interval '1 day' + a.r_minutes * interval '8 hours'
               WHEN NOT a.multi THEN a.order_date + (1 + a.r_minutes * 20) * interval '1 minute'
               WHEN NOT a.is_counted THEN a.order_date + (1 + a.r_minutes * 10) * interval '1 minute'
               ELSE a.order_date + (a.j - 1) * interval '30 days' + (15 + a.r_minutes * 60) * interval '1 minute'
           END AS payment_date
    FROM pay_amounts a
) AS planned
ORDER BY payment_date, order_id, k;
