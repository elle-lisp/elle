//! `Api` value accessors, getters, type checks, and keyword helpers.

use super::*;

impl Api {
    // ── Accessor helpers ──────────────────────────────────────────

    pub fn get_int(&self, v: ElleValue) -> Option<i64> {
        let mut out = 0i64;
        if (self.as_int)(v, &mut out) {
            Some(out)
        } else {
            None
        }
    }

    pub fn get_float(&self, v: ElleValue) -> Option<f64> {
        let mut out = 0f64;
        if (self.as_float)(v, &mut out) {
            Some(out)
        } else {
            None
        }
    }

    pub fn get_bool(&self, v: ElleValue) -> Option<bool> {
        match (self.as_bool)(v) {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }

    pub fn get_string<'a>(&self, v: ElleValue) -> Option<&'a str> {
        let mut len = 0usize;
        let ptr = (self.as_string)(v, &mut len);
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len)) })
        }
    }

    pub fn get_bytes<'a>(&self, v: ElleValue) -> Option<&'a [u8]> {
        let mut len = 0usize;
        let ptr = (self.as_bytes)(v, &mut len);
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { std::slice::from_raw_parts(ptr, len) })
        }
    }

    pub fn get_array_len(&self, v: ElleValue) -> Option<usize> {
        let len = (self.array_len)(v);
        if len < 0 {
            None
        } else {
            Some(len as usize)
        }
    }

    pub fn get_array_item(&self, v: ElleValue, idx: usize) -> ElleValue {
        (self.array_get)(v, idx)
    }

    /// Convert a proper list (cons chain) to an immutable array.
    /// Returns `None` if the value is not a proper list.
    ///
    /// Allocates the array into the call's region, so it threads the per-call
    /// `ctx` the primitive received.
    pub fn list_to_array(&self, ctx: *mut ElleCtx, v: ElleValue) -> Option<ElleValue> {
        let result = (self.list_to_array)(ctx, v);
        if self.check_nil(result) {
            None
        } else {
            Some(result)
        }
    }

    pub fn get_struct_field(&self, v: ElleValue, key: &str) -> ElleValue {
        (self.struct_get)(v, key.as_ptr(), key.len())
    }

    pub fn get_struct_len(&self, v: ElleValue) -> Option<usize> {
        let n = (self.struct_len)(v);
        if n < 0 {
            None
        } else {
            Some(n as usize)
        }
    }

    pub fn get_struct_key<'a>(&self, v: ElleValue, idx: usize) -> Option<&'a str> {
        let mut len = 0usize;
        let ptr = (self.struct_key)(v, idx, &mut len);
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len)) })
        }
    }

    pub fn get_struct_value(&self, v: ElleValue, idx: usize) -> ElleValue {
        (self.struct_value)(v, idx)
    }

    /// Iterate struct entries as (key, value) pairs.
    pub fn struct_entries(&self, v: ElleValue) -> Vec<(&str, ElleValue)> {
        let n = match self.get_struct_len(v) {
            Some(n) => n,
            None => return Vec::new(),
        };
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            if let Some(k) = self.get_struct_key(v, i) {
                out.push((k, self.get_struct_value(v, i)));
            }
        }
        out
    }

    pub fn kw_intern(&self, name: &str) -> u64 {
        (self.intern_keyword)(name.as_ptr(), name.len())
    }

    pub fn kw_name<'a>(&self, hash: u64) -> Option<&'a str> {
        let mut len = 0usize;
        let ptr = (self.keyword_name)(hash, &mut len);
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len)) })
        }
    }

    // ── Type predicates ────────────────────────────────────────

    pub fn check_string(&self, v: ElleValue) -> bool {
        (self.is_string)(v)
    }
    pub fn check_keyword(&self, v: ElleValue) -> bool {
        (self.is_keyword)(v)
    }
    pub fn check_bytes(&self, v: ElleValue) -> bool {
        (self.is_bytes)(v)
    }
    pub fn check_array(&self, v: ElleValue) -> bool {
        (self.is_array)(v)
    }
    pub fn check_struct(&self, v: ElleValue) -> bool {
        (self.is_struct)(v)
    }
    pub fn check_int(&self, v: ElleValue) -> bool {
        (self.is_int)(v)
    }
    pub fn check_float(&self, v: ElleValue) -> bool {
        (self.is_float)(v)
    }
    pub fn check_bool(&self, v: ElleValue) -> bool {
        (self.is_bool_val)(v)
    }
    pub fn check_nil(&self, v: ElleValue) -> bool {
        (self.is_nil)(v)
    }
    pub fn check_external(&self, v: ElleValue) -> bool {
        (self.is_external)(v)
    }

    pub fn get_keyword_name<'a>(&self, v: ElleValue) -> Option<&'a str> {
        let mut len = 0usize;
        let ptr = (self.as_keyword_name)(v, &mut len);
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len)) })
        }
    }

    pub fn eq(&self, a: ElleValue, b: ElleValue) -> bool {
        (self.value_eq)(a, b)
    }

    pub fn type_name<'a>(&self, v: ElleValue) -> &'a str {
        let mut len = 0usize;
        let ptr = (self.type_name_of)(v, &mut len);
        if ptr.is_null() || len == 0 {
            "unknown"
        } else {
            unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len)) }
        }
    }
}
