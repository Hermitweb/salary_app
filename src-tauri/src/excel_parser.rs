//! Excel 解析：员工列表与工资表。

use crate::error::AppError;
use crate::models::{Employee, FieldConfig, FieldState};
use calamine::{Data, Reader, open_workbook_auto};
use std::collections::HashMap;
use std::path::Path;

/// 工号列关键字。
///
/// 员工表与工资表共用同一份关键字，保证两边对「哪一列是工号」的判断一致。
pub const ID_KEYWORDS: &[&str] = &["工号", "员工号", "员工编号", "账号"];
/// 密码列关键字。
///
/// 与 Python 版保持一致：表里没有「密码」列时，依次回落到
/// 手机号 / 联系电话 / 紧急联系人（号码）——初始密码就是 HR 登记的紧急联系人号码。
pub const PASSWORD_KEYWORDS: &[&str] =
    &["密码", "手机号", "联系电话", "紧急联系人", "紧急联系人号码"];
/// 姓名列关键字。
const NAME_KEYWORDS: &[&str] = &["姓名", "名字"];
/// 部门列关键字。
const DEPT_KEYWORDS: &[&str] = &["部门", "科室"];
/// 在职状态列关键字。
const STATUS_KEYWORDS: &[&str] = &["离职", "在职状态", "状态"];

/// 视为「已离职」的状态取值（比较时忽略大小写）。
const RESIGNED_VALUES: &[&str] = &[
    "是",
    "已离职",
    "离职",
    "y",
    "1",
    "true",
    "yes",
    "已注销",
    "注销",
];

/// 判断某个在职状态取值是否表示「已离职」。
///
/// 登录校验与控制面板的账号列表都用这个判断：
/// 已停用的账号不该出现在面板的账号下拉里。
pub fn is_resigned(status: &str) -> bool {
    let status = status.trim();
    RESIGNED_VALUES.contains(&status.to_lowercase().as_str()) || RESIGNED_VALUES.contains(&status)
}

/// 查找列索引：先精确匹配，命中不了再退回到模糊匹配。
///
/// 精确匹配优先是必要的：表里同时出现「工号」和「工号状态」时应当选前者，
/// 只做模糊匹配会挑到「工号状态」。同理「离职日期」与「在职状态」并存时，
/// 应按「在职状态」判断在职状态，而不是把离职日期当成状态列。
///
/// 同一轮匹配内关键字顺序优先于列顺序，关键字表因此按业务优先级排列：
/// 「密码」总是优先于「手机号」。
pub fn find_column_index(headers: &[String], keywords: &[&str]) -> Option<usize> {
    // 第一轮：精确匹配
    for keyword in keywords {
        if let Some(idx) = headers.iter().position(|header| header.trim() == *keyword) {
            return Some(idx);
        }
    }

    // 第二轮：模糊匹配
    for keyword in keywords {
        if let Some(idx) = headers.iter().position(|header| header.contains(keyword)) {
            return Some(idx);
        }
    }

    None
}

/// 把单元格值转成文本。
///
/// 数字会去掉多余的尾随零（`8000.00` -> `8000`），但保留业务需要的小数（`1234.50` -> `1234.5`）。
pub fn cell_to_string(value: &Data) -> String {
    match value {
        Data::String(s) => s.clone(),
        Data::Int(i) => i.to_string(),
        Data::Float(n) => {
            let formatted = format!("{n:.2}");
            let trimmed = formatted.trim_end_matches('0');
            let trimmed = trimmed.strip_suffix('.').unwrap_or(trimmed);
            trimmed.to_string()
        }
        Data::Bool(b) => if *b { "是" } else { "否" }.to_string(),
        Data::DateTime(dt) => dt.to_string(),
        Data::DateTimeIso(s) => s.clone(),
        Data::DurationIso(s) => s.clone(),
        Data::Error(_) => String::new(),
        Data::Empty => String::new(),
    }
}

/// 规范化字段名：去掉首尾空白。
///
/// 工资表表头与 `fields.txt` 里的键都必须走这个口径：
/// 表头带一个空格而配置键不带的话，字段显示配置就会「改了不生效」。
pub fn normalize_field_name(name: &str) -> String {
    name.trim().to_string()
}

/// 登录字段映射：网页登录的账号列与密码列分别是哪一列。
///
/// 控制面板从表头里选好之后写进 `data/login_fields.json`；
/// 两列都允许为空——为空表示沿用关键字探测（[`ID_KEYWORDS`] / [`PASSWORD_KEYWORDS`]）。
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LoginFieldMapping {
    /// 账号列的表头名。
    #[serde(default)]
    pub account_field: String,
    /// 密码列的表头名。
    #[serde(default)]
    pub password_field: String,
}

impl LoginFieldMapping {
    /// 映射是否一个字段都没配。
    pub fn is_empty(&self) -> bool {
        self.account_field.trim().is_empty() && self.password_field.trim().is_empty()
    }
}

/// 读取登录字段映射；文件不存在或损坏时返回空映射（走关键字探测）。
pub fn read_login_mapping() -> LoginFieldMapping {
    let path = crate::paths::login_fields_file();
    let Ok(text) = std::fs::read_to_string(path) else {
        return LoginFieldMapping::default();
    };

    serde_json::from_str(&text).unwrap_or_default()
}

/// 保存登录字段映射（原子写，写一半被读到会拿到半截 JSON）。
pub fn write_login_mapping(mapping: &LoginFieldMapping) -> Result<(), String> {
    let path = crate::paths::login_fields_file();

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("创建数据目录失败: {error}"))?;
    }

    let text = serde_json::to_string_pretty(mapping)
        .map_err(|error| format!("序列化登录字段映射失败: {error}"))?;

    let temp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&temp, text).map_err(|error| format!("写入登录字段映射失败: {error}"))?;
    std::fs::rename(&temp, path).map_err(|error| format!("保存登录字段映射失败: {error}"))?;

    Ok(())
}

/// 按表头名找列：规范化后精确匹配。
///
/// 登录字段映射里存的是表头名，加载员工表时用这个名字对号入座。
fn find_named_column(headers: &[String], name: &str) -> Option<usize> {
    let name = normalize_field_name(name);
    if name.is_empty() {
        return None;
    }

    headers
        .iter()
        .position(|header| normalize_field_name(header) == name)
}

/// 解析员工表时先看映射、再看关键字。
///
/// 映射里配了但表里没有这一列时退回关键字探测：
/// 控制面板的候选列表来自同一张表，正常不会出现这种情况，
/// 但表头被人改掉时宁可退回默认行为，也不要让登录整个挂掉。
fn resolve_login_columns(
    headers: &[String],
    mapping: &LoginFieldMapping,
) -> (Option<usize>, Option<usize>) {
    let employee_id_idx = match find_named_column(headers, &mapping.account_field) {
        Some(index) => Some(index),
        None => find_column_index(headers, ID_KEYWORDS),
    };

    let password_idx = match find_named_column(headers, &mapping.password_field) {
        Some(index) => Some(index),
        None => find_column_index(headers, PASSWORD_KEYWORDS),
    };

    (employee_id_idx, password_idx)
}

/// 比较两个工号是否指向同一个人。
///
/// Excel 里 `001` 可能被存成文本 `001`，也可能被存成数字 `1`，
/// 两种情况下用户输入的工号都应当匹配。
///
/// 登录校验与工资查询都走这个函数：两边的匹配口径必须一致，
/// 否则会出现「能登录但查不到工资记录」的情况。
pub fn ids_match(a: &str, b: &str) -> bool {
    let a = a.trim();
    let b = b.trim();

    if a == b {
        return true;
    }

    match (a.parse::<f64>(), b.parse::<f64>()) {
        (Ok(x), Ok(y)) => (x - y).abs() < f64::EPSILON,
        _ => false,
    }
}

/// 员工表解析结果。
#[derive(Debug)]
pub struct EmployeesData {
    pub rows: Vec<HashMap<String, String>>,
    pub headers: Vec<String>,
    pub employee_id_idx: Option<usize>,
    pub password_idx: Option<usize>,
    pub name_idx: Option<usize>,
    pub dept_idx: Option<usize>,
    pub status_idx: Option<usize>,
    /// 员工表的结构类型。
    pub mode: SheetMode,
}

impl EmployeesData {
    /// 空结果。
    fn empty() -> Self {
        Self {
            rows: Vec::new(),
            headers: Vec::new(),
            employee_id_idx: None,
            password_idx: None,
            name_idx: None,
            dept_idx: None,
            status_idx: None,
            mode: SheetMode::Standard,
        }
    }

    /// 取出某一列的值。
    fn value<'a>(
        &'a self,
        row: &'a HashMap<String, String>,
        idx: Option<usize>,
    ) -> Option<&'a str> {
        let idx = idx?;
        let header = self.headers.get(idx)?;
        row.get(header).map(|value| value.trim())
    }
}

/// 员工表的两种结构。
///
/// - 标准模式：第 1 行是表头（工号 / 姓名 / 部门 …），第 2 行起每行一名员工；
/// - 原始模式：每一列是一名员工，前 4 行依次是工号、密码、姓名、部门。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetMode {
    Standard,
    Raw,
}

/// 判断表头是否属于标准模式。
///
/// 与 Python 版一致：只要出现任意一个标准字段名就按标准模式处理，
/// 否则按原始模式（每列一名员工）解析。
const STANDARD_FIELDS: &[&str] = &[
    "员工号",
    "工号",
    "姓名",
    "部门",
    "手机号",
    "密码",
    "管理员",
    "联系电话",
    "紧急联系人",
    "紧急联系人号码",
    "离职",
    "离职否",
];

/// 判断表头是否属于标准模式。
pub fn has_standard_headers(headers: &[String]) -> bool {
    headers
        .iter()
        .any(|header| STANDARD_FIELDS.iter().any(|field| header.contains(field)))
}

/// 严格定位「密码」列：只认真正的口令列，不回落到手机号等列。
///
/// 批量把口令列换成哈希值时必须用这个：把「紧急联系人号码」当成口令列去哈希，
/// 会把员工表里的联系方式全部毁掉。
pub fn find_strict_password_column(headers: &[String]) -> Option<usize> {
    headers
        .iter()
        .position(|header| header.trim().contains("密码"))
}

/// 读取整个工作表为二维文本数组。
pub fn read_grid(path: &Path) -> Result<Vec<Vec<String>>, AppError> {
    let mut workbook = open_workbook_auto(path)
        .map_err(|e| AppError::IoError(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))?;

    let worksheet = workbook
        .worksheet_range_at(0)
        .ok_or_else(|| AppError::InternalError(format!("工作簿中没有工作表: {}", path.display())))?
        .map_err(|e| {
            AppError::IoError(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e.to_string(),
            ))
        })?;

    Ok(worksheet
        .rows()
        .map(|row| row.iter().map(cell_to_string).collect())
        .collect())
}

/// 在表头中定位工号列。
///
/// 员工表与工资表共用同一套关键字，两边对「哪一列是工号」的判断才不会分叉。
pub fn find_employee_id_column(headers: &[String]) -> Option<usize> {
    find_column_index(headers, ID_KEYWORDS)
}

/// Excel 数据解析器。
pub struct ExcelParser;

impl ExcelParser {
    /// 加载员工表。
    ///
    /// 同时支持标准模式与原始模式（见 [`SheetMode`]）。
    pub fn load_employees(path: &Path) -> Result<EmployeesData, AppError> {
        if !path.exists() {
            return Err(AppError::EmployeeFileNotFound(path.display().to_string()));
        }

        let grid = read_grid(path)?;

        let Some(first_row) = grid.first() else {
            return Ok(EmployeesData::empty());
        };

        let has_standard = has_standard_headers(first_row);

        if has_standard {
            Self::load_standard(&grid)
        } else {
            Self::load_raw(&grid)
        }
    }

    /// 标准模式：首行表头，每行一名员工。
    fn load_standard(grid: &[Vec<String>]) -> Result<EmployeesData, AppError> {
        let headers = grid[0].clone();

        // 登录字段映射优先：控制面板里选了哪列当账号/密码，就以它为准
        let mapping = read_login_mapping();
        let (employee_id_idx, password_idx) = resolve_login_columns(&headers, &mapping);

        let name_idx = find_column_index(&headers, NAME_KEYWORDS);
        let dept_idx = find_column_index(&headers, DEPT_KEYWORDS);
        let status_idx = find_column_index(&headers, STATUS_KEYWORDS);

        if employee_id_idx.is_none() {
            return Err(AppError::AuthenticationFailed(
                "员工表缺少工号列，登录已禁用，请联系管理员检查员工列表.xlsx".to_string(),
            ));
        }

        if password_idx.is_none() {
            return Err(AppError::AuthenticationFailed(
                "员工表缺少密码列，登录已禁用，请联系管理员检查员工列表.xlsx".to_string(),
            ));
        }

        let mut rows: Vec<HashMap<String, String>> = Vec::new();

        for row in grid.iter().skip(1) {
            let mut map = HashMap::new();

            for (idx, cell) in row.iter().enumerate() {
                if let Some(header) = headers.get(idx) {
                    if !header.trim().is_empty() {
                        map.insert(header.clone(), cell.clone());
                    }
                }
            }

            if !map.is_empty() {
                rows.push(map);
            }
        }

        Ok(EmployeesData {
            rows,
            headers,
            employee_id_idx,
            password_idx,
            name_idx,
            dept_idx,
            status_idx,
            mode: SheetMode::Standard,
        })
    }

    /// 原始模式：每列一名员工，前 4 行分别是工号、密码、姓名、部门。
    ///
    /// 第 5 行历史上是管理员标记。网页端已不再区分管理员，该行直接忽略——
    /// 但前 4 行的位置不能动，否则工号/密码/姓名/部门会整体错位。
    fn load_raw(grid: &[Vec<String>]) -> Result<EmployeesData, AppError> {
        const RAW_HEADERS: [&str; 4] = ["工号", "密码", "姓名", "部门"];

        let headers: Vec<String> = RAW_HEADERS.iter().map(|h| h.to_string()).collect();
        let width = grid.iter().map(|row| row.len()).max().unwrap_or(0);

        let mut rows: Vec<HashMap<String, String>> = Vec::new();

        for column in 0..width {
            let mut map = HashMap::new();

            for (offset, header) in headers.iter().enumerate() {
                let value = grid
                    .get(offset)
                    .and_then(|row| row.get(column))
                    .map(|value| value.trim().to_string())
                    .unwrap_or_default();

                if !value.is_empty() {
                    map.insert(header.clone(), value);
                }
            }

            // 没有工号的列不是员工
            if map.contains_key("工号") {
                rows.push(map);
            }
        }

        Ok(EmployeesData {
            rows,
            headers,
            employee_id_idx: Some(0),
            password_idx: Some(1),
            name_idx: Some(2),
            dept_idx: Some(3),
            status_idx: None,
            mode: SheetMode::Raw,
        })
    }

    /// 校验工号与密码。
    ///
    /// 密码错误与工号不存在返回同一句提示，避免暴露工号是否存在。
    pub fn authenticate(
        employees: &EmployeesData,
        username: &str,
        password: &str,
    ) -> Result<Employee, AppError> {
        let username = username.trim();

        for row in &employees.rows {
            let Some(employee_id) = employees.value(row, employees.employee_id_idx) else {
                continue;
            };

            if !ids_match(employee_id, username) {
                continue;
            }

            let Some(stored_password) = employees.value(row, employees.password_idx) else {
                continue;
            };

            // 存量口令是明文，升级后的口令是 PBKDF2 哈希，
            // 两种格式都由 `password::verify` 分派，登录行为对员工无感。
            if !crate::password::verify(stored_password, password) {
                continue;
            }

            // 已离职的账号不允许登录
            if let Some(status) = employees.value(row, employees.status_idx) {
                if is_resigned(status) {
                    return Err(AppError::AuthenticationFailed("该账号已停用".to_string()));
                }
            }

            let name = employees
                .value(row, employees.name_idx)
                .filter(|value| !value.is_empty())
                .map(str::to_string);

            let dept = employees
                .value(row, employees.dept_idx)
                .filter(|value| !value.is_empty())
                .map(str::to_string);

            return Ok(Employee {
                employee_id: employee_id.to_string(),
                name,
                dept,
            });
        }

        Err(AppError::AuthenticationFailed("工号或密码错误".to_string()))
    }

    /// 从 `fields.txt` 加载字段显示配置。
    pub fn load_field_config(path: &Path) -> Result<FieldConfig, AppError> {
        let mut config = HashMap::new();

        if !path.exists() {
            return Ok(FieldConfig { config });
        }

        let content = std::fs::read_to_string(path).map_err(AppError::IoError)?;

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let Some((key, value)) = line.split_once('=') else {
                continue;
            };

            let key = normalize_field_name(key);
            let value = value.trim().to_lowercase();

            let state = match value.as_str() {
                "show" | "显示" | "true" | "1" | "yes" | "y" | "是" => FieldState::Show,
                "hide" | "隐藏" | "false" | "0" | "no" | "n" | "否" => FieldState::Hide,
                _ => FieldState::Auto,
            };

            config.insert(key, state);
        }

        Ok(FieldConfig { config })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_names_are_normalized_the_same_way_everywhere() {
        assert_eq!(normalize_field_name(" 绩效 "), "绩效");
        assert_eq!(normalize_field_name("绩效"), "绩效");
    }

    #[test]
    fn config_keys_match_headers_regardless_of_surrounding_whitespace() {
        // 配置里的键带空白时，去空白后的表头也要能命中
        let path = std::env::temp_dir().join("salary-field-config-key-test.txt");
        std::fs::write(&path, "  绩效  =  隐藏  \n").unwrap();

        let config = ExcelParser::load_field_config(&path).expect("配置应当可读");
        assert_eq!(config.config.get("绩效"), Some(&FieldState::Hide));
        assert!(
            !config.should_show("绩效", "1200"),
            "隐藏应当对去空白后的表头生效"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn column_index_prefers_exact_match() {
        let headers = vec!["账号状态".to_string(), "工号".to_string()];
        // 「账号状态」虽然包含「账号」，但精确匹配的「工号」应优先
        assert_eq!(find_column_index(&headers, ID_KEYWORDS), Some(1));
    }

    #[test]
    fn column_index_falls_back_to_fuzzy_match() {
        // 没有精确命中时才退回到模糊匹配
        let headers = vec!["员工工号".to_string(), "姓名".to_string()];
        assert_eq!(find_column_index(&headers, ID_KEYWORDS), Some(0));
    }

    #[test]
    fn column_index_prefers_exact_over_earlier_fuzzy_keyword() {
        // 「离职日期」模糊命中关键字「离职」，但精确的「在职状态」才是状态列
        let headers = vec!["离职日期".to_string(), "在职状态".to_string()];
        assert_eq!(find_column_index(&headers, STATUS_KEYWORDS), Some(1));
    }

    #[test]
    fn employee_id_column_is_shared_by_employee_and_salary_tables() {
        // 工资表的工号列名叫「员工号」，员工表叫「工号」——两边都要能识别
        let salary_headers = vec!["员工号".to_string(), "姓名".to_string()];
        let employee_headers = vec!["工号".to_string(), "姓名".to_string()];

        assert_eq!(find_employee_id_column(&salary_headers), Some(0));
        assert_eq!(find_employee_id_column(&employee_headers), Some(0));
    }

    #[test]
    fn login_mapping_overrides_keyword_detection() {
        // 控制面板选了「登录名」当账号列、「口令」当密码列：
        // 解析员工表时就应当按这两列来，而不是关键字探测的结果
        let headers = vec![
            "登录名".to_string(),
            "口令".to_string(),
            "工号".to_string(),
            "紧急联系人号码".to_string(),
        ];

        let mapping = LoginFieldMapping {
            account_field: "登录名".to_string(),
            password_field: "口令".to_string(),
        };

        let (id_idx, password_idx) = resolve_login_columns(&headers, &mapping);
        assert_eq!(id_idx, Some(0), "账号列应当按映射取「登录名」");
        assert_eq!(password_idx, Some(1), "密码列应当按映射取「口令」");
    }

    #[test]
    fn login_mapping_falls_back_when_column_missing() {
        // 映射里配的字段在表里不存在时，退回关键字探测而不是报错
        let headers = vec!["工号".to_string(), "紧急联系人号码".to_string()];

        let mapping = LoginFieldMapping {
            account_field: "不存在的列".to_string(),
            password_field: "也不存在".to_string(),
        };

        let (id_idx, password_idx) = resolve_login_columns(&headers, &mapping);
        assert_eq!(id_idx, Some(0));
        assert_eq!(password_idx, Some(1));
    }

    #[test]
    fn login_mapping_serializes_with_empty_fields() {
        // 空映射（未配置）也要能落盘、能读回——
        // 控制面板保存「仅改密码字段」这类部分配置时不该被序列化卡住
        let empty = LoginFieldMapping::default();
        let text = serde_json::to_string(&empty).unwrap();
        assert_eq!(empty, serde_json::from_str(&text).unwrap());

        let partial = LoginFieldMapping {
            account_field: "工号".to_string(),
            password_field: String::new(),
        };
        let text = serde_json::to_string(&partial).unwrap();
        let parsed: LoginFieldMapping = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed.account_field, "工号");
        assert!(parsed.password_field.is_empty());
    }

    #[test]
    fn password_column_falls_back_to_contact_number() {
        let headers = vec![
            "工号".to_string(),
            "姓名".to_string(),
            "紧急联系人号码".to_string(),
        ];
        assert_eq!(find_column_index(&headers, PASSWORD_KEYWORDS), Some(2));
    }

    #[test]
    fn password_column_wins_over_phone_number() {
        let headers = vec!["工号".to_string(), "手机号".to_string(), "密码".to_string()];
        // 「密码」关键字排在前面，优先于「手机号」
        assert_eq!(find_column_index(&headers, PASSWORD_KEYWORDS), Some(2));
    }

    #[test]
    fn raw_mode_employee_sheet_is_supported() {
        // 原始模式：每列一名员工，没有表头
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("员工列表-原始模式.xlsx");

        if !fixture.exists() {
            eprintln!("测试数据缺失，跳过：{}", fixture.display());
            return;
        }

        let employees = ExcelParser::load_employees(&fixture).expect("应当能读取原始模式员工表");
        assert_eq!(employees.mode, SheetMode::Raw);
        assert_eq!(employees.rows.len(), 3);

        let employee =
            ExcelParser::authenticate(&employees, "002", "salary2026").expect("应当能登录");
        assert_eq!(employee.employee_id, "002");
        assert_eq!(employee.name.as_deref(), Some("李四"));
        assert_eq!(employee.dept.as_deref(), Some("财务部"));

        // 第 5 行原本是管理员标记，现在不再区分管理员：
        // 那一列的员工照样能登录，姓名部门也不能因为少读一行而错位。
        let third = ExcelParser::authenticate(&employees, "003", "admin2026").expect("应当能登录");
        assert_eq!(third.employee_id, "003");
        assert_eq!(third.name.as_deref(), Some("王五"));
        assert_eq!(third.dept.as_deref(), Some("人力资源部"));

        // 密码错误不通过
        assert!(ExcelParser::authenticate(&employees, "002", "wrong").is_err());
    }

    #[test]
    fn hashed_passwords_are_accepted() {
        // 哈希口令与明文口令可以混在同一张员工表里
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("员工列表-哈希口令.xlsx");

        if !fixture.exists() {
            eprintln!("测试数据缺失，跳过：{}", fixture.display());
            return;
        }

        let employees = ExcelParser::load_employees(&fixture).expect("应当能读取哈希口令员工表");

        // 哈希口令可以正常登录
        let employee =
            ExcelParser::authenticate(&employees, "001", "salary2026").expect("应当能登录");
        assert_eq!(employee.employee_id, "001");
        assert_eq!(employee.name.as_deref(), Some("张三"));

        // 口令错误不通过
        assert!(ExcelParser::authenticate(&employees, "001", "wrong-password").is_err());

        // 每名员工的口令互相独立
        assert!(ExcelParser::authenticate(&employees, "002", "another-secret").is_ok());
        assert!(ExcelParser::authenticate(&employees, "002", "salary2026").is_err());

        // 同一张表里的明文口令依然能用
        assert!(ExcelParser::authenticate(&employees, "004", "plaintext-2026").is_ok());
        assert!(ExcelParser::authenticate(&employees, "004", "salary2026").is_err());
    }

    #[test]
    fn cell_to_string_trims_trailing_zeros() {
        assert_eq!(cell_to_string(&Data::Float(8000.0)), "8000");
        assert_eq!(cell_to_string(&Data::Float(1234.5)), "1234.5");
        assert_eq!(cell_to_string(&Data::Float(0.0)), "0");
        assert_eq!(cell_to_string(&Data::Int(42)), "42");
        assert_eq!(cell_to_string(&Data::Empty), "");
        assert_eq!(cell_to_string(&Data::String("张三".into())), "张三");
    }

    #[test]
    fn ids_match_handles_numeric_padding() {
        assert!(ids_match("001", "001"));
        assert!(ids_match("001", "1"));
        assert!(!ids_match("001", "002"));
        assert!(!ids_match("001", "abc"));
    }
}
