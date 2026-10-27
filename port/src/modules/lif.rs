//! Image::ExifTool::LIF (Image/ExifTool/LIF.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read Leica Image File (LIF) meta information.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::raf_read;
use crate::perl::Value;
use crate::pl::{self, arg, n, re, ret};
use crate::modules::quick_time_stream::check_invocant;

fn s(x: &str) -> Value {
    pl::s(x)
}

fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

//------------------------------------------------------------------------------
// Shorten obscenely long LIF tag names
// Inputs: Tag name
// Returns: Shortened tag name
pub fn shorten_tag_names(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    // (local $_: the substitutions work on a copy of the argument)
    let mut v = arg(&a, 0);
    let sub = |v: &mut Value, pat: &str, rep: &str, g: bool| {
        re(pat, "").subst(v, g, |_| rep.as_bytes().to_vec());
    };
    sub(&mut v, r"DescriptionDimensionsDimensionDescription", "Dimensions", false);
    sub(&mut v, r"DescriptionChannelsChannelDescription", "Channel", false);
    sub(&mut v, r"ShutterListShutter", "Shutter", false);
    sub(&mut v, r"SettingDefinition", "Setting", false);
    sub(&mut v, r"AdditionalZPositionListAdditionalZPosition", "AdditionalZPosition", false);
    sub(&mut v, r"LMSDataContainerHeader", "", true);
    sub(&mut v, r"FilterWheelWheel", "FilterWheel", false);
    sub(&mut v, r"FilterWheelFilter", "FilterWheel", false);
    sub(&mut v, r"DetectorListDetector", "Detector", false);
    sub(&mut v, r"OnlineDyeSeparationOnlineDyeSeparation", "OnlineDyeSeparation", false);
    sub(&mut v, r"AotfListAotf", "Aotf", false);
    sub(&mut v, r"SettingAotfLaserLineSetting", "SettingAotfLaser", false);
    sub(&mut v, r"DataROISetROISet", "DataROISet", false);
    sub(&mut v, r"AdditionalZPosition", "AddZPos", false);
    sub(&mut v, r"FRAPplusBlock_FRAPBlock_FRAP_PrePost_Info", "FRAP_", false);
    sub(&mut v, r"FRAPplusBlock_FRAPBlock_FRAP_(Master)?", "FRAP_", false);
    sub(&mut v, r"LDM_Block_SequentialLDM_Block_Sequential_", "LDM_", false);
    sub(&mut v, r"ATLConfocalSetting", "ATLConfocal", false);
    sub(&mut v, r"LaserArrayLaser", "Laser", false);
    sub(&mut v, r"LDM_Master", "LDM_", false);
    sub(&mut v, r"(List)?ATLConfocal", "ATL_", false);
    sub(&mut v, r"Separation", "Sep", false);
    sub(&mut v, r"BleachPointsElement", "BleachPoint", false);
    sub(&mut v, r"BeamPositionBeamPosition", "BeamPosition", false);
    sub(&mut v, r"DataROISetPossible(ROI)?", "DataROISet", false);
    sub(&mut v, r"RoiElementChildrenElementDataROISingle(Roi)?", "Roi", false);
    sub(&mut v, r"InfoLaserLineSettingArrayLaserLineSetting", "LastLineSetting", false);
    sub(&mut v, r"FilterWheelWheelNameFilterName", "FilterWheelFilterName", false);
    sub(&mut v, r"LUT_ListLut", "Lut", false);
    sub(&mut v, r"ROI_ListRoiRoidata", "ROI_", false);
    sub(&mut v, r"LaserLineSettingArrayLaserLineSetting", "LaserLineSetting", false);
    ret(v)
}

//------------------------------------------------------------------------------
// Extract metadata from a LIF image
// Inputs: 0) ExifTool object reference, 1) dirInfo reference
// Returns: 1 on success, 0 if this wasn't a valid LIF file
pub fn process_lif(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let mut buff = Value::Undef;

    // verify this is a valid LIF file
    pl::at(103);
    check_invocant(it, &raf, "Read")?;
    if !(pl::num_eq(&raf_read(it, &raf, &mut buff, n(15))?, &n(15)) && re(r"^\x70\0{3}.{4}\x2a.{4}<\0", "s").is_match(&buff)) {
        return ret(n(0));
    }

    pl::method(it, &et, "SetFileType", vec![])?;
    call(it, "SetByteOrder", vec![s("II")])?;

    let size = call(it, "Get32u", vec![pl::sref(buff.clone()), n(4)])?; // XML chunk size
    let len = pl::mul(&call(it, "Get32u", vec![pl::sref(buff.clone()), n(9)])?, &n(2)); // XML data length

    if pl::lt(&size, &len) {
        pl::method(it, &et, "Error", vec![s("Corrupted LIF XML block")])?;
        return ret(n(1));
    }
    if pl::gt(&size, &n(100000000)) {
        pl::method(it, &et, "Error", vec![s("LIF XML block too large")])?;
        return ret(n(1));
    }

    if !(pl::method(it, &raf, "Seek", vec![n(-2), n(1)])?.truthy() && pl::num_eq(&raf_read(it, &raf, &mut buff, len.clone())?, &len)) {
        pl::method(it, &et, "Error", vec![s("Truncated LIF XML block")])?;
        return ret(n(1));
    }

    let tag_table_ptr = call(it, "GetTagTable", vec![s("Image::ExifTool::LIF::Main")])?;

    // convert from UTF16 to UTF8
    let xml = call(it, "Decode", vec![et.clone(), buff, s("UTF16"), s("II"), s("UTF8")])?;

    let dir_info2 = Value::new_hash();
    dir_info2.set("DataPt", pl::sref(xml));

    et.set(
        "XmpIgnoreProps",
        Value::new_array(vec![s("LMSDataContainerHeader"), s("Element"), s("Children"), s("Data"), s("Image"), s("Attachment")]),
    );
    et.set("ShortenXmpTags", Value::Code(it.sub_ref("Image::ExifTool::LIF::ShortenTagNames")));

    pl::method(it, &et, "ProcessDirectory", vec![dir_info2, tag_table_ptr])?;

    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::LIF::ShortenTagNames", shorten_tag_names);
    it.define_native("Image::ExifTool::LIF::ProcessLIF", process_lif);
}
