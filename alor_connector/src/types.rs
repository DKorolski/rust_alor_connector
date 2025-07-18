use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

/// Котировка best-bid/ask + last
#[derive(Debug, Clone)]
pub struct Quote {
    pub symbol:             String,         // d["symbol"]                    
    pub exchange:           String,         // d["exchange"]                  
    pub description:        Option<String>, // d["description"]               
    pub prev_close_price:   Option<f64>,    // d["prev_close_price"]          
    pub last_price:         f64,            // d["last_price"]                
    pub last_price_timestamp: i64,          // d["last_price_timestamp"]      
    pub high_price:         Option<f64>,    // d["high_price"]                
    pub low_price:          Option<f64>,    // d["low_price"]                 
    pub volume:             Option<i64>,    // d["volume"]                    
    pub open_interest:      Option<i64>,    // d["open_interest"]             
    pub ask:                f64,            // d["ask"]                       
    pub bid:                f64,            // d["bid"]                       
    pub ask_vol:            Option<i64>,    // d["ask_vol"]                   
    pub bid_vol:            Option<i64>,    // d["bid_vol"]                   
    pub total_ask_vol:      Option<i64>,    // d["total_ask_vol"]             
    pub total_bid_vol:      Option<i64>,    // d["total_bid_vol"]             
    pub ob_ms_timestamp:    Option<i64>,    // d["ob_ms_timestamp"]           
    pub open_price:         Option<f64>,    // d["open_price"]                
    pub yield_field:        Option<f64>,    // d["yield"]                     
    pub lot_size:           Option<i64>,    // d["lotsize"] or d["lotSize"]   
    pub lot_value:          Option<i64>,    // d["lotvalue"]                  
    pub face_value:         Option<i64>,    // d["facevalue"]                 
    pub security_type:      Option<String>, // d["type"]                      
    pub change:             Option<f64>,    // d["change"]                    
    pub change_percent:     Option<f64>,    // d["change_percent"]            
}

/// Бар свечи
#[derive(Debug, Clone)]
pub struct Bar {
    pub symbol:   String,
    pub exchange: String,
    pub tf:       String,
    pub open:     f64,
    pub high:     f64,
    pub low:      f64,
    pub close:    f64,
    pub volume:   f64,
    pub ts:       i64,
}

#[derive(Clone)]
pub enum OpCode {
    QuotesSubscribe,
    BarsGetAndSubscribe,
}

#[derive(Clone)]
pub struct SubscriptionInfo {
    pub symbol:   String,
    pub exchange: String,
    pub kind:     OpCode,
    pub tf:       Option<String>,
}

#[derive(Debug, Clone)]
pub enum WsEvent {
    Quote(Quote),
    Bar  (Bar),
}

impl WsEvent {
    /// Попытка распарсить v["data"] → Quote / Bar по info.kind
    pub fn try_from_json(
        _guid: Uuid,
        v: &Value,
        info: &SubscriptionInfo,
    ) -> Option<Self> {
        let d = v.get("data")?;

        match info.kind {
            OpCode::QuotesSubscribe => {
                // Извлекаем все поля, которые теперь есть в Quote
                let symbol    = d.get("symbol")?.as_str()?.to_string();
                let exchange  = d.get("exchange")?.as_str()?.to_string();
                let description        = d.get("description").and_then(Value::as_str).map(str::to_string);
                let prev_close_price   = d.get("prev_close_price").and_then(Value::as_f64);
                let last_price         = d.get("last_price")?.as_f64()?;
                let last_price_ts      = d.get("last_price_timestamp")?.as_i64()?;
                let high_price         = d.get("high_price").and_then(Value::as_f64);
                let low_price          = d.get("low_price").and_then(Value::as_f64);
                let volume             = d.get("volume").and_then(Value::as_i64);
                let open_interest      = d.get("open_interest").and_then(Value::as_i64);
                let ask                = d.get("ask")?.as_f64()?;
                let bid                = d.get("bid")?.as_f64()?;
                let ask_vol            = d.get("ask_vol").and_then(Value::as_i64);
                let bid_vol            = d.get("bid_vol").and_then(Value::as_i64);
                let total_ask_vol      = d.get("total_ask_vol").and_then(Value::as_i64);
                let total_bid_vol      = d.get("total_bid_vol").and_then(Value::as_i64);
                let ob_ms_ts           = d.get("ob_ms_timestamp").and_then(Value::as_i64);
                let open_price         = d.get("open_price").and_then(Value::as_f64);
                let yield_field             = d.get("yield").and_then(Value::as_f64);
                let lot_size           = d.get("lotsize").or_else(|| d.get("lot_size")).and_then(Value::as_i64);
                let lot_value          = d.get("lotvalue").and_then(Value::as_i64);
                let face_value         = d.get("facevalue").and_then(Value::as_i64);
                let security_type      = d.get("type").and_then(Value::as_str).map(str::to_string);
                let change             = d.get("change").and_then(Value::as_f64);
                let change_percent     = d.get("change_percent").and_then(Value::as_f64);

                Some(WsEvent::Quote(Quote {
                    symbol,
                    exchange,
                    description,
                    prev_close_price,
                    last_price,
                    last_price_timestamp: last_price_ts,
                    high_price,
                    low_price,
                    volume,
                    open_interest,
                    ask,
                    bid,
                    ask_vol,
                    bid_vol,
                    total_ask_vol,
                    total_bid_vol,
                    ob_ms_timestamp: ob_ms_ts,
                    open_price,
                    yield_field,
                    lot_size,
                    lot_value,
                    face_value,
                    security_type,
                    change,
                    change_percent,
                }))
            }

            OpCode::BarsGetAndSubscribe => {
                let tf      = info.tf.clone()?;
                let symbol  = info.symbol.clone();
                let exchange= info.exchange.clone();
                let open    = d.get("open")?.as_f64()?;
                let high    = d.get("high")?.as_f64()?;
                let low     = d.get("low")?.as_f64()?;
                let close   = d.get("close")?.as_f64()?;
                let volume  = d.get("volume")?.as_f64()?;
                let ts      = d.get("time")?.as_i64()?;

                Some(WsEvent::Bar(Bar {
                    symbol,
                    exchange,
                    tf,
                    open,
                    high,
                    low,
                    close,
                    volume,
                    ts,
                }))
            }
        }
    }
}

