use super::filter_operator::FilterOperator;
use crate::planner::query_tools::QueryTools;
use crate::planner::sql_evaluator::MemberSymbol;
use crate::planner::sql_templates::filter::FilterTemplates;
use crate::planner::{evaluate_with_context, VisitorContext};
use cubenativeutils::CubeError;
use lazy_static::lazy_static;
use regex::Regex;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterType {
    Dimension,
    Measure,
}

pub struct BaseFilter {
    query_tools: Rc<QueryTools>,
    member_evaluator: Rc<MemberSymbol>,
    #[allow(dead_code)]
    filter_type: FilterType,
    filter_operator: FilterOperator,
    values: Vec<Option<String>>,
    templates: FilterTemplates,
}

impl PartialEq for BaseFilter {
    fn eq(&self, other: &Self) -> bool {
        self.filter_type == other.filter_type
            && self.filter_operator == other.filter_operator
            && self.values == other.values
    }
}

lazy_static! {
    static ref DATE_TIME_LOCAL_MS_RE: Regex =
        Regex::new(r"^\d\d\d\d-\d\d-\d\dT\d\d:\d\d:\d\d\.\d\d\d$").unwrap();
    static ref DATE_TIME_LOCAL_U_RE: Regex =
        Regex::new(r"^\d\d\d\d-\d\d-\d\dT\d\d:\d\d:\d\d\.\d\d\d\d\d\d$").unwrap();
    static ref DATE_RE: Regex = Regex::new(r"^\d\d\d\d-\d\d-\d\d$").unwrap();
}

impl BaseFilter {
    pub fn try_new(
        query_tools: Rc<QueryTools>,
        member_evaluator: Rc<MemberSymbol>,
        filter_type: FilterType,
        filter_operator: FilterOperator,
        values: Option<Vec<Option<String>>>,
    ) -> Result<Rc<Self>, CubeError> {
        let templates = FilterTemplates::new(query_tools.templates_render());
        let values = if let Some(values) = values {
            values
        } else {
            vec![]
        };
        Ok(Rc::new(Self {
            query_tools,
            member_evaluator,
            filter_type,
            filter_operator,
            values,
            templates,
        }))
    }

    pub fn change_operator(
        &self,
        filter_operator: FilterOperator,
        values: Vec<Option<String>>,
    ) -> Rc<Self> {
        Rc::new(Self {
            query_tools: self.query_tools.clone(),
            member_evaluator: self.member_evaluator.clone(),
            filter_type: self.filter_type.clone(),
            filter_operator,
            values,
            templates: self.templates.clone(),
        })
    }

    pub fn member_evaluator(&self) -> &Rc<MemberSymbol> {
        &self.member_evaluator
    }

    pub fn values(&self) -> &Vec<Option<String>> {
        &self.values
    }

    pub fn filter_operator(&self) -> &FilterOperator {
        &self.filter_operator
    }

    pub fn member_name(&self) -> String {
        self.member_evaluator.full_name()
    }

    pub fn to_sql(&self, context: Rc<VisitorContext>) -> Result<String, CubeError> {
        let member_sql =
            evaluate_with_context(&self.member_evaluator, self.query_tools.clone(), context)?;
        let res = match self.filter_operator {
            FilterOperator::Equal => self.equals_where(&member_sql)?,
            FilterOperator::NotEqual => self.not_equals_where(&member_sql)?,
            FilterOperator::InDateRange => self.in_date_range(&member_sql)?,
            FilterOperator::InDateRangeExtended => self.in_date_range_extended(&member_sql)?,
            FilterOperator::In => self.in_where(&member_sql)?,
            FilterOperator::NotIn => self.not_in_where(&member_sql)?,
            FilterOperator::Set => self.set_where(&member_sql)?,
            FilterOperator::NotSet => self.not_set_where(&member_sql)?,
            FilterOperator::Gt => self.gt_where(&member_sql)?,
            FilterOperator::Gte => self.gte_where(&member_sql)?,
            FilterOperator::Lt => self.lt_where(&member_sql)?,
            FilterOperator::Lte => self.lte_where(&member_sql)?,
            FilterOperator::Contains => self.contains_where(&member_sql)?,
            FilterOperator::NotContains => self.not_contains_where(&member_sql)?,
            FilterOperator::StartsWith => self.starts_with_where(&member_sql)?,
            FilterOperator::NotStartsWith => self.not_starts_with_where(&member_sql)?,
            FilterOperator::EndsWith => self.ends_with_where(&member_sql)?,
            FilterOperator::NotEndsWith => self.not_ends_with_where(&member_sql)?,
        };
        Ok(res)
    }

    fn equals_where(&self, member_sql: &str) -> Result<String, CubeError> {
        let need_null_check = self.is_need_null_chek(false);
        if self.is_array_value() {
            self.templates.in_where(
                member_sql.to_string(),
                self.filter_and_allocate_values(),
                need_null_check,
            )
        } else if self.is_values_contains_null() {
            self.templates.not_set_where(member_sql.to_string())
        } else {
            self.templates
                .equals(member_sql.to_string(), self.first_param()?, need_null_check)
        }
    }

    fn not_equals_where(&self, member_sql: &str) -> Result<String, CubeError> {
        let need_null_check = self.is_need_null_chek(true);
        if self.is_array_value() {
            self.templates.not_in_where(
                member_sql.to_string(),
                self.filter_and_allocate_values(),
                need_null_check,
            )
        } else if self.is_values_contains_null() {
            self.templates.set_where(member_sql.to_string())
        } else {
            self.templates
                .not_equals(member_sql.to_string(), self.first_param()?, need_null_check)
        }
    }

    fn in_date_range(&self, member_sql: &str) -> Result<String, CubeError> {
        let (from, to) = self.allocate_date_params()?;
        self.templates
            .time_range_filter(member_sql.to_string(), from, to)
    }

    fn extend_date_range_bound(
        &self,
        date: String,
        interval: &Option<String>,
        is_sub: bool,
    ) -> Result<String, CubeError> {
        if let Some(interval) = interval {
            if interval != "unbounded" {
                if is_sub {
                    self.templates.sub_interval(date, interval.clone())
                } else {
                    self.templates.add_interval(date, interval.clone())
                }
            } else {
                Ok(date.to_string())
            }
        } else {
            Ok(date.to_string())
        }
    }
    fn in_date_range_extended(&self, member_sql: &str) -> Result<String, CubeError> {
        let (from, to) = self.allocate_date_params()?;

        let from = if self.values.len() >= 3 {
            self.extend_date_range_bound(from, &self.values[2], true)?
        } else {
            from
        };

        let to = if self.values.len() >= 4 {
            self.extend_date_range_bound(to, &self.values[3], false)?
        } else {
            to
        };

        self.templates
            .time_range_filter(member_sql.to_string(), from, to)
    }

    fn in_where(&self, member_sql: &str) -> Result<String, CubeError> {
        let need_null_check = self.is_need_null_chek(false);
        self.templates.in_where(
            member_sql.to_string(),
            self.filter_and_allocate_values(),
            need_null_check,
        )
    }

    fn not_in_where(&self, member_sql: &str) -> Result<String, CubeError> {
        let need_null_check = self.is_need_null_chek(true);
        self.templates.not_in_where(
            member_sql.to_string(),
            self.filter_and_allocate_values(),
            need_null_check,
        )
    }

    fn set_where(&self, member_sql: &str) -> Result<String, CubeError> {
        self.templates.set_where(member_sql.to_string())
    }

    fn not_set_where(&self, member_sql: &str) -> Result<String, CubeError> {
        self.templates.not_set_where(member_sql.to_string())
    }

    fn gt_where(&self, member_sql: &str) -> Result<String, CubeError> {
        self.templates
            .gt(member_sql.to_string(), self.first_param()?)
    }

    fn gte_where(&self, member_sql: &str) -> Result<String, CubeError> {
        self.templates
            .gte(member_sql.to_string(), self.first_param()?)
    }

    fn lt_where(&self, member_sql: &str) -> Result<String, CubeError> {
        self.templates
            .lt(member_sql.to_string(), self.first_param()?)
    }

    fn lte_where(&self, member_sql: &str) -> Result<String, CubeError> {
        self.templates
            .lte(member_sql.to_string(), self.first_param()?)
    }

    fn contains_where(&self, member_sql: &str) -> Result<String, CubeError> {
        self.like_or_where(member_sql, false, true, true)
    }

    fn not_contains_where(&self, member_sql: &str) -> Result<String, CubeError> {
        self.like_or_where(member_sql, true, true, true)
    }

    fn starts_with_where(&self, member_sql: &str) -> Result<String, CubeError> {
        self.like_or_where(member_sql, false, false, true)
    }

    fn not_starts_with_where(&self, member_sql: &str) -> Result<String, CubeError> {
        self.like_or_where(member_sql, true, false, true)
    }

    fn ends_with_where(&self, member_sql: &str) -> Result<String, CubeError> {
        self.like_or_where(member_sql, false, true, false)
    }

    fn not_ends_with_where(&self, member_sql: &str) -> Result<String, CubeError> {
        self.like_or_where(member_sql, true, true, false)
    }

    fn like_or_where(
        &self,
        member_sql: &str,
        not: bool,
        start_wild: bool,
        end_wild: bool,
    ) -> Result<String, CubeError> {
        let values = self.filter_and_allocate_values();
        let like_parts = values
            .into_iter()
            .map(|v| {
                self.templates
                    .ilike(member_sql, &v, start_wild, end_wild, not)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let logical_symbol = if not { " AND " } else { " OR " };
        let null_check = if self.is_need_null_chek(not) {
            self.templates.or_is_null_check(member_sql.to_string())?
        } else {
            "".to_string()
        };
        Ok(format!(
            "({}){}",
            like_parts.join(logical_symbol),
            null_check
        ))
    }

    fn allocate_date_params(&self) -> Result<(String, String), CubeError> {
        if self.values.len() >= 2 {
            let from = if let Some(from_str) = &self.values[0] {
                self.query_tools
                    .base_tools()
                    .in_db_time_zone(self.format_from_date(&from_str)?)?
            } else {
                return Err(CubeError::user(format!(
                    "Arguments for date range is not valid"
                )));
            };

            let to = if let Some(to_str) = &self.values[1] {
                self.query_tools
                    .base_tools()
                    .in_db_time_zone(self.format_to_date(&to_str)?)?
            } else {
                return Err(CubeError::user(format!(
                    "Arguments for date range is not valid"
                )));
            };
            let from = self.allocate_timestamp_param(&from);
            let to = self.allocate_timestamp_param(&to);
            Ok((from, to))
        } else {
            Err(CubeError::user(format!(
                "2 arguments expected for date range, got {}",
                self.values.len()
            )))
        }
    }

    fn format_from_date(&self, date: &str) -> Result<String, CubeError> {
        let precision = self.query_tools.base_tools().timestamp_precision()?;
        if precision == 3 {
            if DATE_TIME_LOCAL_MS_RE.is_match(date) {
                return Ok(date.to_string());
            }
        } else if precision == 6 {
            if date.len() == 23 && DATE_TIME_LOCAL_MS_RE.is_match(date) {
                return Ok(format!("{}000", date));
            } else if date.len() == 26 && DATE_TIME_LOCAL_U_RE.is_match(date) {
                return Ok(date.to_string());
            }
        } else {
            return Err(CubeError::user(format!(
                "Unsupported timestamp precision: {}",
                precision
            )));
        }

        if DATE_RE.is_match(date) {
            return Ok(format!(
                "{}T00:00:00.{}",
                date,
                "0".repeat(precision as usize)
            ));
        }
        //FIXME chrono don't support parsing date without specified format
        Err(CubeError::user(format!(
            "Unsupported date format: {}",
            date
        )))
    }

    fn format_to_date(&self, date: &str) -> Result<String, CubeError> {
        let precision = self.query_tools.base_tools().timestamp_precision()?;
        if precision == 3 {
            if DATE_TIME_LOCAL_MS_RE.is_match(date) {
                return Ok(date.to_string());
            }
        } else if precision == 6 {
            if date.len() == 23 && DATE_TIME_LOCAL_MS_RE.is_match(date) {
                if date.ends_with(".999") {
                    return Ok(format!("{}999", date));
                }
                return Ok(format!("{}000", date));
            } else if date.len() == 26 && DATE_TIME_LOCAL_U_RE.is_match(date) {
                return Ok(date.to_string());
            }
        } else {
            return Err(CubeError::user(format!(
                "Unsupported timestamp precision: {}",
                precision
            )));
        }

        if DATE_RE.is_match(date) {
            return Ok(format!(
                "{}T23:59:59.{}",
                date,
                "9".repeat(precision as usize)
            ));
        }
        //FIXME chrono don't support parsing date without specified format
        Err(CubeError::user(format!(
            "Unsupported date format: {}",
            date
        )))
    }

    fn allocate_param(&self, param: &str) -> String {
        self.query_tools.allocate_param(param)
    }

    fn allocate_timestamp_param(&self, param: &str) -> String {
        let placeholder = self.query_tools.allocate_param(param);
        format!("{}::timestamptz", placeholder)
    }

    fn first_param(&self) -> Result<String, CubeError> {
        if self.values.is_empty() {
            Err(CubeError::user(format!(
                "Expected one parameter but nothing found"
            )))
        } else {
            if let Some(value) = &self.values[0] {
                Ok(self.allocate_param(value))
            } else {
                Ok("NULL".to_string())
            }
        }
    }

    fn is_need_null_chek(&self, is_not: bool) -> bool {
        let contains_null = self.is_values_contains_null();
        if is_not {
            !contains_null
        } else {
            contains_null
        }
    }

    fn is_values_contains_null(&self) -> bool {
        self.values.iter().any(|v| v.is_none())
    }

    fn is_array_value(&self) -> bool {
        self.values.len() > 1
    }

    fn filter_and_allocate_values(&self) -> Vec<String> {
        self.values
            .iter()
            .filter_map(|v| v.as_ref().map(|v| self.allocate_param(&v)))
            .collect::<Vec<_>>()
    }
}
