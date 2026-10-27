//! Image::ExifTool::ZISRAW (Image/ExifTool/ZISRAW.pm), ported sub by sub (see docs/port/PORTING.md).
//!
//! Read ZISRAW (CZI) meta information.

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::random_access::{cell, raf_read_cell};
use crate::perl::Value;
use crate::pl::{self, arg, cap, n, re, ret};

fn s(x: &str) -> Value {
    pl::s(x)
}

fn call(it: &mut Interp, name: &str, args: Vec<Value>) -> ER<Value> {
    pl::call(it, &format!("Image::ExifTool::{name}"), args)
}

//------------------------------------------------------------------------------
// Shorten obscenely long CZI tag names
// Inputs: Tag name
// Returns: Shortened tag name
pub fn shorten_tag_names(_it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    // (local $_: the substitutions work on a copy of the argument)
    let mut v = arg(&a, 0);
    let sub = |v: &mut Value, pat: &str, rep: &str, g: bool| {
        re(pat, "").subst(v, g, |_| rep.as_bytes().to_vec());
    };
    sub(&mut v, r"^HardwareSetting", "", false);
    sub(&mut v, r"^DevicesDevice", "Device", false);
    sub(&mut v, r"LightPathNode", "", true);
    sub(&mut v, r"Successors", "", true);
    sub(&mut v, r"ExperimentExperiment", "Experiment", true);
    sub(&mut v, r"ObjectivesObjective", "Objective", false);
    sub(&mut v, r"ChannelsChannel", "Channel", false);
    sub(&mut v, r"TubeLensesTubeLens", "TubeLens", false);
    sub(&mut v, r"^ExperimentHardwareSettingsPoolHardwareSetting", "HardwareSetting", false);
    sub(&mut v, r"SharpnessMeasureSetSharpnessMeasure", "Sharpness", false);
    sub(&mut v, r"FocusSetupAutofocusSetup", "Autofocus", false);
    sub(&mut v, r"TracksTrack", "Track", false);
    sub(&mut v, r"ChannelRefsChannelRef", "ChannelRef", false);
    sub(&mut v, r"ChangerChanger", "Changer", false);
    sub(&mut v, r"ElementsChangerElement", "Changer", false);
    sub(&mut v, r"ChangerElements", "Changer", false);
    sub(&mut v, r"ContrastChangerContrast", "Contrast", false);
    sub(&mut v, r"KeyFunctionsKeyFunction", "KeyFunction", false);
    sub(&mut v, r"ManagerContrastManager(Contrast)?", "ManagerContrast", false);
    sub(&mut v, r"ObjectiveChangerObjective", "ObjectiveChanger", false);
    sub(&mut v, r"ManagerLightManager", "ManagerLight", false);
    sub(&mut v, r"WavelengthAreasWavelengthArea", "WavelengthArea", false);
    sub(&mut v, r"ReflectorChangerReflector", "ReflectorChanger", false);
    sub(&mut v, r"^StageStageAxesStageAxis", "StageAxis", false);
    sub(&mut v, r"ShutterChangerShutter", "ShutterChanger", false);
    sub(&mut v, r"OnOffChangerOnOff", "OnOffChanger", false);
    sub(&mut v, r"UnsharpMaskStateUnsharpMask", "UnsharpMask", false);
    sub(&mut v, r"Acquisition", "Acq", false);
    sub(&mut v, r"Continuous", "Cont", false);
    sub(&mut v, r"Resolution", "Res", false);
    sub(&mut v, r"Experiment", "Expt", true);
    sub(&mut v, r"Threshold", "Thresh", false);
    sub(&mut v, r"Reference", "Ref", false);
    sub(&mut v, r"Magnification", "Mag", false);
    sub(&mut v, r"Original", "Orig", false);
    sub(&mut v, r"FocusSetupFocusStrategySetup", "Focus", false);
    sub(&mut v, r"ParametersParameter", "Parameter", false);
    sub(&mut v, r"IntervalInfo", "Interval", false);
    sub(&mut v, r"ExptBlocksAcqBlock", "AcqBlock", false);
    sub(&mut v, r"MicroscopesMicroscope", "Microscope", false);
    sub(&mut v, r"TimeSeriesInterval", "TimeSeries", false);
    re(r"Interval(.*Interval)", "").subst(&mut v, false, |c| [b"".as_slice(), &cap(c, 1).to_bytes(), b"".as_slice()].concat());
    sub(&mut v, r"SingleTileRegionsSingleTileRegion", "SingleTileRegion", false);
    sub(&mut v, r"AcquisitionMode", "", false);
    sub(&mut v, r"DetectorsDetector", "Detector", false);
    sub(&mut v, r"Setup", "", false);
    sub(&mut v, r"Setting", "", false);
    sub(&mut v, r"TrackTrack", "Track", false);
    sub(&mut v, r"AnalogOutMaximumsAnalogOutMaximum", "AnalogOutMaximum", false);
    sub(&mut v, r"AnalogOutMinimumsAnalogOutMinimum", "AnalogOutMinimum", false);
    sub(&mut v, r"DigitalOutLabelsDigitalOutLabelLabel", "DigitalOutLabelLabel", false);
    sub(&mut v, r"(VivaTomeOpticalSectionInformation)+VivaTomeOpticalSectionInformation", "VivaTomeOpticalSectionInformation", false);
    sub(&mut v, r"FocusDefiniteFocus", "FocusDefinite", false);
    sub(&mut v, r"ChangerChanger", "Changer", false);
    sub(&mut v, r"Calibration", "Cal", false);
    sub(&mut v, r"LightSwitchChangerRLTLSwitch", "LightSwitchChangerRLTL", false);
    sub(&mut v, r"Parameters", "", false);
    sub(&mut v, r"Fluorescence", "Fluor", false);
    sub(&mut v, r"CameraGeometryCameraGeometry", "CameraGeometry", false);
    sub(&mut v, r"CameraCamera", "Camera", false);
    sub(&mut v, r"DetectorsCamera", "Camera", false);
    sub(&mut v, r"FilterChangerLeftChangerEmissionFilter", "LeftChangerEmissionFilter", false);
    sub(&mut v, r"SwitchingStatesSwitchingState", "SwitchingState", false);
    sub(&mut v, r"Information", "Info", false);
    sub(&mut v, r"SubDimensions?", "", true);
    sub(&mut v, r"Setups?", "", false);
    sub(&mut v, r"Parameters?", "", false);
    sub(&mut v, r"Calculate", "Calc", false);
    sub(&mut v, r"Visibility", "Vis", false);
    sub(&mut v, r"Orientation", "Orient", false);
    sub(&mut v, r"ListItems", "Items", false);
    sub(&mut v, r"Increment", "Incr", false);
    sub(&mut v, r"Parameter", "Param", false);
    sub(&mut v, r"(ParfocalParcentralValues)+ParfocalParcentralValue", "Parcentral", false);
    sub(&mut v, r"ParcentralParcentral", "Parcentral", false);
    sub(&mut v, r"CorrFocusCorrection", "FocusCorr", false);
    sub(&mut v, r"(ApoTomeDepthInfo)+Element", "ApoTomeDepth", false);
    sub(&mut v, r"(ApoTomeClickStopInfo)+Element", "ApoTomeClickStop", false);
    sub(&mut v, r"DepthDepth", "Depth", false);
    sub(&mut v, r"(Devices?)+Device", "Device", false);
    sub(&mut v, r"(BeamPathNode)+", "BeamPathNode", false);
    sub(&mut v, r"BeamPathsBeamPath", "BeamPath", true);
    sub(&mut v, r"BeamPathBeamPath", "BeamPath", true);
    sub(&mut v, r"Configuration", "Config", false);
    sub(&mut v, r"StageAxesStageAxis", "StageAxis", false);
    sub(&mut v, r"RangesRange", "Range", false);
    sub(&mut v, r"DataGridDatasGridData(Grid)?", "DataGrid", false);
    sub(&mut v, r"DataMicroscopeDatasMicroscopeData(Microscope)?", "DataMicroscope", false);
    sub(&mut v, r"DataWegaDatasWegaData", "DataWega", false);
    sub(&mut v, r"ClickStopPositionsClickStopPosition", "ClickStopPosition", false);
    sub(&mut v, r"LightSourcess?LightSource(Settings)?(LightSource)?", "LightSource", false);
    sub(&mut v, r"FilterSetsFilterSet", "FilterSet", false);
    sub(&mut v, r"EmissionFiltersEmissionFilter", "EmissionFilter", false);
    sub(&mut v, r"ExcitationFiltersExcitationFilter", "ExcitationFilter", false);
    sub(&mut v, r"FiltersFilter", "Filter", false);
    sub(&mut v, r"DichroicsDichroic", "Dichronic", false);
    sub(&mut v, r"WavelengthsWavelength", "Wavelength", false);
    sub(&mut v, r"MultiTrackSetup", "MultiTrack", false);
    sub(&mut v, r"TrackTrack", "Track", false);
    sub(&mut v, r"DataGrabberSetup", "DataGrabber", false);
    sub(&mut v, r"CameraFrameSetup", "CameraFrame", false);
    sub(&mut v, r"TimeSeries(TimeSeries|Setups)", "TimeSeries", false);
    sub(&mut v, r"FocusFocus", "Focus", false);
    sub(&mut v, r"FocusAutofocus", "Autofocus", false);
    re(r"Focus(Hardware|Software)(Autofocus)+", "").subst(&mut v, false, |c| [b"Autofocus".as_slice(), &cap(c, 1).to_bytes(), b"".as_slice()].concat());
    sub(&mut v, r"AutofocusAutofocus", "Autofocus", false);
    ret(v)
}

//------------------------------------------------------------------------------
// Extract metadata from a ZISRAW (CZI) image
// Inputs: 0) ExifTool object reference, 1) dirInfo reference
// Returns: 1 on success, 0 if this wasn't a valid CZI file
pub fn process_czi(it: &mut Interp, a: Vec<Value>, _w: Want) -> ER<Vec<Value>> {
    let (et, dir_info) = (arg(&a, 0), arg(&a, 1));
    let raf = dir_info.get("RAF");
    let buff = cell(Value::Undef);
    let buff_pt = Value::ScalarRef(buff.clone());

    // verify this is a valid CZI file
    if !pl::num_eq(&raf_read_cell(it, &raf, &buff, n(100))?, &n(100)) {
        return ret(n(0));
    }
    if !re(r"^ZISRAWFILE\0{6}", "").is_match(&buff.borrow()) {
        return ret(n(0));
    }
    pl::method(it, &et, "SetFileType", vec![])?;
    call(it, "SetByteOrder", vec![s("II")])?;
    let dir_info2 = Value::new_hash();
    dir_info2.set("DataPt", buff_pt.clone());
    dir_info2.set("DirStart", n(0));
    dir_info2.set("DirLen", pl::length(&buff.borrow()));
    let mut tag_table_ptr = call(it, "GetTagTable", vec![s("Image::ExifTool::ZISRAW::Main")])?;
    pl::method(it, &et, "ProcessDirectory", vec![dir_info2.clone(), tag_table_ptr])?;

    // read the metadata section
    let pos = call(it, "Get64u", vec![buff_pt.clone(), n(92)])?;
    if !pos.truthy() {
        return ret(n(1));
    }
    if !pl::method(it, &raf, "Seek", vec![pos, n(0)])?.truthy() {
        pl::method(it, &et, "Warn", vec![s("Error seeking to metadata")])?;
        return ret(n(0));
    }
    if !pl::num_eq(&raf_read_cell(it, &raf, &buff, n(288))?, &n(288)) {
        pl::method(it, &et, "Warn", vec![s("Error reading metadata header")])?;
        return ret(n(0));
    }
    if !re(r"^ZISRAWMETADATA\0\0", "").is_match(&buff.borrow()) {
        pl::method(it, &et, "Warn", vec![s("Invalid metadata header")])?;
        return ret(n(0));
    }
    let len = call(it, "Get32u", vec![buff_pt.clone(), n(32)])?;
    if !pl::lt(&len, &n(200000000)) {
        pl::method(it, &et, "Warn", vec![s("Metadata section too large. Ignoring")])?;
        return ret(n(0));
    }
    if !raf_read_cell(it, &raf, &buff, len)?.truthy() {
        pl::method(it, &et, "Warn", vec![s("Error reading XML metadata")])?;
        return ret(n(0));
    }
    let b = buff.borrow().clone();
    pl::method(it, &et, "FoundTag", vec![s("XML"), b.clone()])?; // extract as a block
    tag_table_ptr = call(it, "GetTagTable", vec![s("Image::ExifTool::XMP::XML")])?;
    dir_info2.set("DirLen", pl::length(&b));
    // shorten tag names somewhat by removing 'ImageDocumentMetadata' prefix from all
    et.set("XmpIgnoreProps", Value::new_array(vec![s("ImageDocument"), s("Metadata"), s("Information")]));
    et.set("ShortenXmpTags", Value::Code(it.sub_ref("Image::ExifTool::ZISRAW::ShortenTagNames")));

    pl::method(it, &et, "ProcessDirectory", vec![dir_info2, tag_table_ptr])?;

    ret(n(1))
}

pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::ZISRAW::ShortenTagNames", shorten_tag_names);
    it.define_native("Image::ExifTool::ZISRAW::ProcessCZI", process_czi);
}
