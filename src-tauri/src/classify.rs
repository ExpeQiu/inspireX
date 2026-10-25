//! 轻量离线地点标签（供 Photos.sqlite GPS 聚合复用）。
//! 完整自建人脸/场景分类不在此模块；优先复用系统相册结果。

struct City {
    name: &'static str,
    #[allow(dead_code)]
    country: &'static str,
    lat: f64,
    lng: f64,
}

static CITIES: &[City] = &[
    City { name: "北京", country: "中国", lat: 39.9042, lng: 116.4074 },
    City { name: "上海", country: "中国", lat: 31.2304, lng: 121.4737 },
    City { name: "广州", country: "中国", lat: 23.1291, lng: 113.2644 },
    City { name: "深圳", country: "中国", lat: 22.5431, lng: 114.0579 },
    City { name: "杭州", country: "中国", lat: 30.2741, lng: 120.1551 },
    City { name: "成都", country: "中国", lat: 30.5728, lng: 104.0668 },
    City { name: "重庆", country: "中国", lat: 29.5630, lng: 106.5516 },
    City { name: "武汉", country: "中国", lat: 30.5928, lng: 114.3055 },
    City { name: "西安", country: "中国", lat: 34.3416, lng: 108.9398 },
    City { name: "南京", country: "中国", lat: 32.0603, lng: 118.7969 },
    City { name: "苏州", country: "中国", lat: 31.2989, lng: 120.5853 },
    City { name: "天津", country: "中国", lat: 39.3434, lng: 117.3616 },
    City { name: "青岛", country: "中国", lat: 36.0671, lng: 120.3826 },
    City { name: "厦门", country: "中国", lat: 24.4798, lng: 118.0894 },
    City { name: "长沙", country: "中国", lat: 28.2282, lng: 112.9388 },
    City { name: "昆明", country: "中国", lat: 25.0389, lng: 102.7183 },
    City { name: "三亚", country: "中国", lat: 18.2528, lng: 109.5119 },
    City { name: "哈尔滨", country: "中国", lat: 45.8038, lng: 126.5349 },
    City { name: "沈阳", country: "中国", lat: 41.8057, lng: 123.4315 },
    City { name: "香港", country: "中国", lat: 22.3193, lng: 114.1694 },
    City { name: "澳门", country: "中国", lat: 22.1987, lng: 113.5439 },
    City { name: "台北", country: "中国", lat: 25.0330, lng: 121.5654 },
    City { name: "东京", country: "日本", lat: 35.6762, lng: 139.6503 },
    City { name: "大阪", country: "日本", lat: 34.6937, lng: 135.5023 },
    City { name: "首尔", country: "韩国", lat: 37.5665, lng: 126.9780 },
    City { name: "新加坡", country: "新加坡", lat: 1.3521, lng: 103.8198 },
    City { name: "曼谷", country: "泰国", lat: 13.7563, lng: 100.5018 },
    City { name: "悉尼", country: "澳大利亚", lat: -33.8688, lng: 151.2093 },
    City { name: "伦敦", country: "英国", lat: 51.5074, lng: -0.1278 },
    City { name: "巴黎", country: "法国", lat: 48.8566, lng: 2.3522 },
    City { name: "柏林", country: "德国", lat: 52.5200, lng: 13.4050 },
    City { name: "纽约", country: "美国", lat: 40.7128, lng: -74.0060 },
    City { name: "洛杉矶", country: "美国", lat: 34.0522, lng: -118.2437 },
    City { name: "旧金山", country: "美国", lat: 37.7749, lng: -122.4194 },
    City { name: "西雅图", country: "美国", lat: 47.6062, lng: -122.3321 },
    City { name: "多伦多", country: "加拿大", lat: 43.6532, lng: -79.3832 },
    City { name: "温哥华", country: "加拿大", lat: 49.2827, lng: -123.1207 },
];

fn haversine_km(lat1: f64, lng1: f64, lat2: f64, lng2: f64) -> f64 {
    let r = 6371.0;
    let dlat = (lat2 - lat1).to_radians();
    let dlng = (lng2 - lng1).to_radians();
    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlng / 2.0).sin().powi(2);
    2.0 * r * a.sqrt().asin()
}

/// 坐标 → 展示用地名；120km 内取城市名，否则「近X」或坐标串。
pub fn place_label(lat: f64, lng: f64) -> String {
    let mut best: Option<(&City, f64)> = None;
    for c in CITIES {
        let d = haversine_km(lat, lng, c.lat, c.lng);
        if best.map(|(_, bd)| d < bd).unwrap_or(true) {
            best = Some((c, d));
        }
    }
    match best {
        Some((c, d)) if d <= 120.0 => c.name.to_string(),
        Some((c, d)) if d <= 400.0 => format!("近{}", c.name),
        _ => format!("{lat:.1},{lng:.1}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shanghai_label() {
        assert_eq!(place_label(31.23, 121.47), "上海");
    }
}
