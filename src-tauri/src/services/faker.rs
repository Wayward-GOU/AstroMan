//! G-05 Faker / 造数变量
//!
//! 统一一套假数据变量，供请求模板、文档示例、Mock 与测试数据集复用。
//!
//! 两种模式：
//! - `Runtime`：每次调用重新取值，用于真实发送 / 压测，保证多轮数据不同；
//! - `Deterministic`：由 `seed` 决定输出，同一种子结果稳定，文档导出与契约快照用它，
//!   避免每次导出的示例值抖动。
//!
//! 不引入 `fake` 这类大依赖：中文姓名库、城市、公司、手机号段全部内置，
//! 随机源用 LCG + UUID 熵，跨平台一致、可测试。

use chrono::Local;

/// 生成模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakerMode {
    Runtime,
    Deterministic,
}

/// 一次生成上下文：模式 + 种子 + 递进计数
///
/// 同一上下文连续生成多个值时，计数会参与 LCG，避免一次渲染里出现重复值；
/// `Deterministic` 下「种子 + 调用序号」完全决定输出。
#[derive(Debug, Clone)]
pub struct Faker {
    mode: FakerMode,
    state: u64,
}

impl Faker {
    /// 运行时模式：用 UUID 作为熵源，每次构造都不同
    pub fn runtime() -> Self {
        let seed = uuid::Uuid::new_v4().as_u128() as u64;
        Self::with_seed(FakerMode::Runtime, seed)
    }

    /// 确定性模式：同一种子反复调用结果一致
    pub fn deterministic(seed: u64) -> Self {
        Self::with_seed(FakerMode::Deterministic, seed)
    }

    /// 文档示例的惯用种子：接口 ID + 字段序号
    pub fn seed_for(request_id: i64, seq: usize) -> u64 {
        (request_id.max(1) as u64)
            .wrapping_mul(131)
            .wrapping_add(seq as u64 * 17)
            .wrapping_add(0x9E37_79B9_7F4A_7C15)
    }

    fn with_seed(mode: FakerMode, seed: u64) -> Self {
        // LCG 全零状态会原地踏步，保证种子非零
        Self {
            mode,
            state: seed | 1,
        }
    }

    fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let mixed = self.state ^ (self.state >> 33);
        mixed.wrapping_mul(0xFF51_AFD7_ED55_8CCD) ^ (mixed >> 29)
    }

    fn range(&mut self, min: i64, max: i64) -> i64 {
        if max <= min {
            return min;
        }
        let span = (max as i128 - min as i128 + 1) as u128;
        (min as i128 + (self.next() as u128 % span) as i128) as i64
    }

    fn pick<'a>(&mut self, items: &'a [&'a str]) -> &'a str {
        if items.is_empty() {
            return "";
        }
        items[(self.next() as usize) % items.len()]
    }

    /// 解析并生成一个 faker 表达式（不含 `faker:` 前缀）
    ///
    /// 未知名称返回 `None`，由调用方决定保留原占位还是报错。
    pub fn value(&mut self, expr: &str) -> Option<String> {
        let expr = expr.trim();
        let (name, args) = match expr.split_once('(') {
            Some((name, rest)) => (name.trim(), Some(rest.trim_end_matches(')').trim())),
            None => (expr, None),
        };
        let lower = name.to_ascii_lowercase();

        let value = match lower.as_str() {
            "name" | "namezh" | "chinesename" | "username" => {
                format!("{}{}", self.surname(), self.given_name())
            }
            "surname" | "lastname" => self.surname().to_string(),
            "firstname" | "givenname" => self.given_name().to_string(),
            "email" => {
                let prefix = self.pick(&[
                    "user", "demo", "test", "hello", "contact", "dev", "apollo", "astro",
                ]);
                let serial = self.range(10_000, 99_999);
                format!("{prefix}{serial}@{}", self.pick(EMAIL_DOMAINS))
            }
            "phone" | "mobile" => {
                let prefix = self.pick(PHONE_PREFIXES);
                let tail = format!("{:08}", self.range(0, 99_999_999));
                format!("{prefix}{tail}")
            }
            "uuid" | "guid" => self.uuid(),
            "date" => self.date(),
            "datetime" | "time" => self.datetime(),
            "int" | "integer" | "number" => {
                let (min, max) = match args {
                    Some(_) => parse_int_args(args)?,
                    // 无参数时用默认区间，不能把 `int()` 当未知变量
                    None => (1, 1000),
                };
                self.range(min, max).to_string()
            }
            "bool" | "boolean" => {
                if self.next().is_multiple_of(2) {
                    "true".to_string()
                } else {
                    "false".to_string()
                }
            }
            "word" => self.pick(WORDS).to_string(),
            "city" => self.pick(CITIES).to_string(),
            "company" => format!(
                "{}{}有限公司",
                self.pick(COMPANY_PREFIXES),
                self.pick(COMPANY_SUFFIXES)
            ),
            "url" => format!(
                "https://{}/api/{}",
                self.pick(EMAIL_DOMAINS),
                self.pick(WORDS)
            ),
            _ => return None,
        };
        Some(value)
    }

    fn surname(&mut self) -> &'static str {
        self.pick(SURNAMES)
    }

    fn given_name(&mut self) -> String {
        let first = self.pick(GIVEN_NAMES);
        if self.next().is_multiple_of(3) {
            let second = self.pick(GIVEN_NAMES);
            format!("{first}{second}")
        } else {
            first.to_string()
        }
    }

    fn uuid(&mut self) -> String {
        let a = self.next();
        let b = self.next();
        let hex = format!("{a:016x}{b:016x}");
        format!(
            "{}-{}-4{}-a{}-{}",
            &hex[0..8],
            &hex[8..12],
            &hex[13..16],
            &hex[17..20],
            &hex[20..32]
        )
    }

    fn date(&mut self) -> String {
        match self.mode {
            FakerMode::Runtime => Local::now().format("%Y-%m-%d").to_string(),
            FakerMode::Deterministic => fixed_date(self.next()),
        }
    }

    fn datetime(&mut self) -> String {
        match self.mode {
            FakerMode::Runtime => Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string(),
            FakerMode::Deterministic => format!("{}T12:00:00+08:00", fixed_date(self.next())),
        }
    }
}

/// 确定性日期：在 2026 年内取一天，保证同一份文档反复导出完全一致
fn fixed_date(seed: u64) -> String {
    let offset = seed % 365;
    let mut base = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).expect("固定日期合法");
    base += chrono::Duration::days(offset as i64);
    base.format("%Y-%m-%d").to_string()
}

fn parse_int_args(args: Option<&str>) -> Option<(i64, i64)> {
    let raw = args?;
    let mut parts = raw.split(',').map(str::trim);
    let min = parts.next()?.parse::<i64>().ok()?;
    let max = parts.next()?.parse::<i64>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((min.min(max), min.max(max)))
}

/// 便捷入口：运行时生成一次
pub fn runtime_value(expr: &str) -> Option<String> {
    Faker::runtime().value(expr)
}

const SURNAMES: &[&str] = &[
    "王", "李", "张", "刘", "陈", "杨", "黄", "赵", "吴", "周", "徐", "孙", "马", "朱", "胡", "郭",
    "何", "高", "林", "罗", "郑", "梁", "谢", "宋", "唐", "许", "韩", "冯", "邓", "曹", "彭", "曾",
    "肖", "田", "董", "袁", "潘", "于", "蒋", "蔡", "余", "杜", "叶", "程", "苏", "魏", "吕", "丁",
    "任", "沈", "姚", "卢", "姜", "崔", "钟", "谭", "陆", "汪", "范", "金",
];

const GIVEN_NAMES: &[&str] = &[
    "伟", "芳", "娜", "敏", "静", "丽", "强", "磊", "军", "洋", "勇", "艳", "杰", "娟", "涛", "明",
    "超", "霞", "平", "刚", "华", "玉兰", "萍", "红", "燕", "彬", "鹏", "辉", "建国", "志强",
    "晓明", "雨欣", "子涵", "浩然", "欣怡", "梓萱", "宇轩", "思远", "佳琪", "一诺",
];

const CITIES: &[&str] = &[
    "北京",
    "上海",
    "广州",
    "深圳",
    "杭州",
    "成都",
    "武汉",
    "南京",
    "西安",
    "苏州",
    "天津",
    "重庆",
    "长沙",
    "郑州",
    "青岛",
    "宁波",
    "厦门",
    "合肥",
    "福州",
    "济南",
    "大连",
    "沈阳",
    "昆明",
    "哈尔滨",
    "石家庄",
    "太原",
    "南昌",
    "贵阳",
    "南宁",
    "兰州",
    "海口",
    "银川",
];

const COMPANY_PREFIXES: &[&str] = &[
    "华夏", "云图", "星海", "智联", "恒创", "天工", "博远", "新元", "聚力", "联诚", "启明", "远航",
    "数联", "光年", "万象",
];

const COMPANY_SUFFIXES: &[&str] = &[
    "科技",
    "网络",
    "信息技术",
    "数据",
    "智能",
    "软件",
    "电子",
    "传媒",
    "物流",
    "咨询",
];

const EMAIL_DOMAINS: &[&str] = &[
    "example.com",
    "example.cn",
    "test.com",
    "mail.example.com",
    "demo.example.com",
];

const PHONE_PREFIXES: &[&str] = &[
    "130", "131", "132", "133", "135", "136", "137", "138", "139", "150", "151", "152", "153",
    "155", "156", "157", "158", "159", "180", "181", "182", "183", "185", "186", "187", "188",
    "189",
];

const WORDS: &[&str] = &[
    "alpha", "beta", "gamma", "delta", "orbit", "lunar", "solar", "nova", "comet", "pixel",
    "signal", "matrix", "vector", "cloud", "river", "stone", "light", "frame", "apple", "orange",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_results_are_stable() {
        let seed = Faker::seed_for(7, 2);
        let first = Faker::deterministic(seed).value("email").unwrap();
        let second = Faker::deterministic(seed).value("email").unwrap();
        assert_eq!(first, second);
        assert!(first.ends_with("@example.com") || first.contains('@'));
    }

    #[test]
    fn runtime_results_differ_between_calls() {
        let mut values = std::collections::HashSet::new();
        for _ in 0..12 {
            values.insert(Faker::runtime().value("uuid").unwrap());
        }
        assert!(values.len() > 1, "运行时模式应产生不同值");
    }

    #[test]
    fn int_respects_bounds_and_swaps_reversed_range() {
        let mut faker = Faker::deterministic(42);
        for _ in 0..50 {
            let value: i64 = faker.value("int(5,9)").unwrap().parse().unwrap();
            assert!((5..=9).contains(&value));
        }
        let value: i64 = faker.value("int(9,5)").unwrap().parse().unwrap();
        assert!((5..=9).contains(&value));
        assert_eq!(faker.value("int(3,3)").unwrap(), "3");
    }

    #[test]
    fn known_names_produce_expected_shapes() {
        let mut faker = Faker::deterministic(9);
        assert!(faker.value("phone").unwrap().len() == 11);
        assert_eq!(faker.value("uuid").unwrap().len(), 36);
        assert!(faker.value("date").unwrap().starts_with("2026-"));
        assert!(faker.value("datetime").unwrap().contains("T12:00:00+08:00"));
        assert!(["true", "false"].contains(&faker.value("bool").unwrap().as_str()));
        assert!(faker.value("company").unwrap().ends_with("有限公司"));
        assert!(CITIES.contains(&faker.value("city").unwrap().as_str()));
    }

    #[test]
    fn unknown_name_returns_none() {
        assert!(Faker::deterministic(1).value("nope").is_none());
        assert!(Faker::deterministic(1).value("int(a,b)").is_none());
    }

    #[test]
    fn deterministic_values_differ_per_field() {
        let request_id = 3;
        let names: Vec<String> = (0..6)
            .map(|seq| {
                Faker::deterministic(Faker::seed_for(request_id, seq))
                    .value("name")
                    .unwrap()
            })
            .collect();
        assert!(names.iter().collect::<std::collections::HashSet<_>>().len() > 1);
    }
}
