//! Main-window geometry is an observation, never restored minimized or hidden.
use crate::persistence::ui::WindowGeometry;
use tauri::Manager;
pub(crate) fn visible(mut geometry:WindowGeometry,areas:&[(i32,i32,u32,u32,f64)])->Result<WindowGeometry,String>{
    if !geometry.scale_factor.is_finite()||geometry.scale_factor<=0.0{return Err("Invalid saved display scale.".into());}
    let area=areas.iter().find(|(x,y,w,h,_)|i64::from(geometry.x)<i64::from(*x)+i64::from(*w)&&i64::from(geometry.y)<i64::from(*y)+i64::from(*h)&&i64::from(geometry.x)+i64::from(geometry.width)>i64::from(*x)&&i64::from(geometry.y)+i64::from(geometry.height)>i64::from(*y)).or_else(||areas.first()).ok_or("No display work area is available.")?;
    if area.2==0||area.3==0||!area.4.is_finite()||area.4<=0.0{return Err("Invalid display work area.".into());}
    let scale=area.4/geometry.scale_factor;geometry.width=((geometry.width as f64*scale).round() as u32).clamp(1,area.2);geometry.height=((geometry.height as f64*scale).round() as u32).clamp(1,area.3);
    geometry.x=i64::from(geometry.x).clamp(i64::from(area.0),i64::from(area.0)+i64::from(area.2-geometry.width)) as i32;
    geometry.y=i64::from(geometry.y).clamp(i64::from(area.1),i64::from(area.1)+i64::from(area.3-geometry.height)) as i32;geometry.scale_factor=area.4;Ok(geometry)
}
pub(crate) fn restore(app:&tauri::AppHandle)->Result<(),String>{
    let preferences=&app.state::<crate::NativeGateway>().0.ui_preferences;let row=preferences.read("windows")?;
    apply(app,row.value)
}
fn apply(app:&tauri::AppHandle,value:serde_json::Value)->Result<(),String>{
    if value.is_null(){return Ok(());}let geometry:WindowGeometry=serde_json::from_value(value).map_err(|_|"Invalid saved main-window geometry.")?;
    let window=app.get_webview_window("main").ok_or("Main window is unavailable.")?;
    let areas=window.available_monitors().map_err(|e|e.to_string())?.into_iter().map(|monitor|{let area=monitor.work_area();(area.position.x,area.position.y,area.size.width,area.size.height,monitor.scale_factor())}).collect::<Vec<_>>();
    let geometry=visible(geometry,&areas)?;
    if window.is_maximized().map_err(|e|e.to_string())?{window.unmaximize().map_err(|e|e.to_string())?;}
    window.set_size(tauri::PhysicalSize::new(geometry.width,geometry.height)).map_err(|e|e.to_string())?;
    window.set_position(tauri::PhysicalPosition::new(geometry.x,geometry.y)).map_err(|e|e.to_string())?;
    if geometry.maximized{window.maximize().map_err(|e|e.to_string())?;}Ok(())
}
pub(crate) fn capture(app:&tauri::AppHandle)->Result<serde_json::Value,String>{
    let window=app.get_webview_window("main").ok_or("Main window is unavailable.")?;
    if window.is_minimized().map_err(|e|e.to_string())?{return Ok(serde_json::Value::Null);}
    let maximized=window.is_maximized().map_err(|e|e.to_string())?;
    if maximized {
        let mut value=app.state::<crate::NativeGateway>().0.ui_preferences.cached("windows");
        if value.is_object(){value["maximized"]=serde_json::json!(true);return Ok(value);}
    }
    let origin=window.outer_position().map_err(|e|e.to_string())?;let size=window.inner_size().map_err(|e|e.to_string())?;
    serde_json::to_value(WindowGeometry{x:origin.x,y:origin.y,width:size.width,height:size.height,scale_factor:window.scale_factor().map_err(|e|e.to_string())?,maximized}).map_err(|_|"Cannot encode window geometry.".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn display_removal_partial_offscreen_and_dpi_changes_keep_every_edge_visible(){
        let geometry=WindowGeometry{x:3000,y:-2000,width:1440,height:960,scale_factor:1.0,maximized:false};let areas=[(-1920,0,1920,1040,1.0),(0,0,1920,1040,1.5)];
        let moved=visible(geometry.clone(),&areas).unwrap();assert_eq!((moved.x,moved.y),(-1440,0));
        let moved=visible(WindowGeometry{x:1800,y:1000,..geometry.clone()},&areas).unwrap();assert_eq!((moved.x,moved.y,moved.width,moved.height),(0,0,1920,1040));assert_eq!(moved.scale_factor,1.5);
        assert!(visible(geometry.clone(),&[]).is_err());assert!(visible(geometry,&[(0,0,0,0,1.0)]).is_err());
    }
}
