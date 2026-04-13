//! Carl Zeiss Image (CZI/ZISRAW) format reader.

use super::misc::mktag;
use crate::error::{Error, Result};
use crate::tag::Tag;
use crate::value::Value;

pub fn read_czi(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 100 || !data.starts_with(b"ZISRAWFILE\x00\x00\x00\x00\x00\x00") {
        return Err(Error::InvalidData("not a ZISRAW/CZI file".into()));
    }

    let mut tags = Vec::new();

    // Binary header fields (little-endian)
    // ZISRAWVersion at offset 0x20: two int32u values
    if data.len() >= 0x28 {
        let major = u32::from_le_bytes([data[0x20], data[0x21], data[0x22], data[0x23]]);
        let minor = u32::from_le_bytes([data[0x24], data[0x25], data[0x26], data[0x27]]);
        let version = format!("{}.{}", major, minor);
        tags.push(mktag(
            "ZISRAW",
            "ZISRAWVersion",
            "ZISRAW Version",
            Value::String(version),
        ));
    }

    // PrimaryFileGUID at offset 0x30: 16 bytes as hex
    if data.len() >= 0x40 {
        let guid = hex_encode(&data[0x30..0x40]);
        tags.push(mktag(
            "ZISRAW",
            "PrimaryFileGUID",
            "Primary File GUID",
            Value::String(guid),
        ));
    }

    // FileGUID at offset 0x40: 16 bytes as hex
    if data.len() >= 0x50 {
        let guid = hex_encode(&data[0x40..0x50]);
        tags.push(mktag(
            "ZISRAW",
            "FileGUID",
            "File GUID",
            Value::String(guid),
        ));
    }

    // Metadata section offset at byte 92 (0x5C): 64-bit LE
    if data.len() >= 100 {
        let meta_off = u64::from_le_bytes([
            data[92], data[93], data[94], data[95], data[96], data[97], data[98], data[99],
        ]) as usize;
        if meta_off > 0 && meta_off + 288 <= data.len() {
            // Check for ZISRAWMETADATA magic
            if &data[meta_off..meta_off + 16] == b"ZISRAWMETADATA\x00\x00" {
                // XML length at offset 32 of metadata segment
                let xml_len = u32::from_le_bytes([
                    data[meta_off + 32],
                    data[meta_off + 33],
                    data[meta_off + 34],
                    data[meta_off + 35],
                ]) as usize;
                let xml_start = meta_off + 288;
                if xml_start + xml_len <= data.len() {
                    let xml_bytes = &data[xml_start..xml_start + xml_len];
                    // Emit XML as binary data tag
                    tags.push(mktag(
                        "ZISRAW",
                        "XML",
                        "XML",
                        Value::String(format!(
                            "(Binary data {} bytes, use -b option to extract)",
                            xml_len
                        )),
                    ));
                    // Parse XML metadata
                    if let Ok(xml_str) = std::str::from_utf8(xml_bytes) {
                        czi_parse_xml(xml_str, &mut tags);
                    }
                }
            }
        }
    }

    Ok(tags)
}

/// Parse CZI XML metadata and extract tags with shortened names.
/// Skips ImageDocument, Metadata, Information path elements (XmpIgnoreProps).
fn czi_parse_xml(xml: &str, tags: &mut Vec<Tag>) {
    use xml::reader::{EventReader, XmlEvent};

    let parser = EventReader::from_str(xml);
    // Path of element names (excluding ignored elements)
    let mut path: Vec<String> = Vec::new();
    // Stack tracking whether each element is ignored
    let mut ignored: Vec<bool> = Vec::new();
    let mut current_text = String::new();
    let mut has_child: Vec<bool> = Vec::new();

    // Elements to ignore in path building
    let ignore_elems = ["ImageDocument", "Metadata", "Information"];

    for event in parser {
        match event {
            Ok(XmlEvent::StartElement {
                name, attributes, ..
            }) => {
                let elem_name = &name.local_name;
                let is_ignored = ignore_elems.contains(&elem_name.as_str());
                ignored.push(is_ignored);
                if let Some(last) = has_child.last_mut() {
                    *last = true;
                }

                if !is_ignored {
                    path.push(elem_name.clone());
                    has_child.push(false);
                    current_text.clear();

                    // Emit attributes as tags
                    let path_str = path.join("");
                    for attr in &attributes {
                        let aname = &attr.name;
                        // Skip xmlns and xsi attributes
                        if aname.prefix.as_deref() == Some("xmlns")
                            || aname.prefix.as_deref() == Some("xsi")
                            || aname.local_name.starts_with("xmlns")
                        {
                            continue;
                        }
                        let raw_tag = format!("{}{}", path_str, aname.local_name);
                        let tag_name = czi_shorten_tag_name(&raw_tag);
                        if !tag_name.is_empty() {
                            let val = attr.value.trim().to_string();
                            tags.push(mktag("ZISRAW", &tag_name, &tag_name, Value::String(val)));
                        }
                    }
                } else {
                    has_child.push(false);
                    current_text.clear();
                }
            }
            Ok(XmlEvent::Characters(text)) | Ok(XmlEvent::CData(text)) => {
                current_text.push_str(&text);
            }
            Ok(XmlEvent::EndElement { .. }) => {
                let is_ignored = ignored.pop().unwrap_or(false);
                let is_leaf = !has_child.pop().unwrap_or(false);

                if !is_ignored {
                    if is_leaf {
                        let text = current_text.trim().to_string();
                        // Only emit leaf text nodes when they have content OR
                        // when the element has no attributes (emit empty string for empty elements with attributes
                        // is handled by attribute processing; don't double-emit)
                        // We emit if: has attributes AND text is empty? No - don't emit if empty
                        // Actually only emit if text is non-empty OR element has attributes-only children
                        // Perl: emit element text as tag; attributes are separate tags
                        // The empty <DeviceRef></DeviceRef> case: has attribute Id (emitted separately),
                        // the element text "" should NOT be emitted as a separate tag
                        // But <StandSpecification>Inverted</> SHOULD be emitted
                        // Rule: only emit leaf text if the text is non-empty
                        if !text.is_empty() {
                            let path_str = path.join("");
                            let tag_name = czi_shorten_tag_name(&path_str);
                            if !tag_name.is_empty() {
                                tags.push(mktag(
                                    "ZISRAW",
                                    &tag_name,
                                    &tag_name,
                                    Value::String(text),
                                ));
                            }
                        }
                    }
                    path.pop();
                } else {
                    // Ignored element - don't pop path (it wasn't pushed)
                }
                current_text.clear();
            }
            _ => {}
        }
    }
}

/// Apply CZI tag name shortening (mirrors Perl's ShortenTagNames).
fn czi_shorten_tag_name(name: &str) -> String {
    let mut s = name.to_string();

    // Apply substitutions in order (mirrors Perl's ShortenTagNames)
    s = s.strip_prefix("HardwareSetting").unwrap_or(&s).to_string();
    s = regex_replace(&s, "^DevicesDevice", "Device");
    s = s.replace("LightPathNode", "");
    s = s.replace("Successors", "");
    s = s.replace("ExperimentExperiment", "Experiment");
    s = regex_replace(&s, "ObjectivesObjective", "Objective");
    s = s.replace("ChannelsChannel", "Channel");
    s = s.replace("TubeLensesTubeLens", "TubeLens");
    s = regex_replace(
        &s,
        "^ExperimentHardwareSettingsPoolHardwareSetting",
        "HardwareSetting",
    );
    s = s.replace("SharpnessMeasureSetSharpnessMeasure", "Sharpness");
    s = s.replace("FocusSetupAutofocusSetup", "Autofocus");
    s = s.replace("TracksTrack", "Track");
    s = s.replace("ChannelRefsChannelRef", "ChannelRef");
    s = s.replace("ChangerChanger", "Changer");
    s = s.replace("ElementsChangerElement", "Changer");
    s = s.replace("ChangerElements", "Changer");
    s = s.replace("ContrastChangerContrast", "Contrast");
    s = s.replace("KeyFunctionsKeyFunction", "KeyFunction");
    s = regex_replace(&s, "ManagerContrastManager(Contrast)?", "ManagerContrast");
    s = s.replace("ObjectiveChangerObjective", "ObjectiveChanger");
    s = s.replace("ManagerLightManager", "ManagerLight");
    s = s.replace("WavelengthAreasWavelengthArea", "WavelengthArea");
    s = s.replace("ReflectorChangerReflector", "ReflectorChanger");
    s = regex_replace(&s, "^StageStageAxesStageAxis", "StageAxis");
    s = s.replace("ShutterChangerShutter", "ShutterChanger");
    s = s.replace("OnOffChangerOnOff", "OnOffChanger");
    s = s.replace("UnsharpMaskStateUnsharpMask", "UnsharpMask");
    s = s.replace("Acquisition", "Acq");
    s = s.replace("Continuous", "Cont");
    s = s.replace("Resolution", "Res");
    s = s.replace("Experiment", "Expt");
    s = s.replace("Threshold", "Thresh");
    s = s.replace("Reference", "Ref");
    s = s.replace("Magnification", "Mag");
    s = s.replace("Original", "Orig");
    s = s.replace("FocusSetupFocusStrategySetup", "Focus");
    s = s.replace("ParametersParameter", "Parameter");
    s = s.replace("IntervalInfo", "Interval");
    s = s.replace("ExptBlocksAcqBlock", "AcqBlock");
    s = s.replace("MicroscopesMicroscope", "Microscope");
    s = s.replace("TimeSeriesInterval", "TimeSeries");
    // s/Interval(.*Interval)/$1/  - complex, handle with loop
    while let Some(idx) = s.find("Interval") {
        let rest = &s[idx + "Interval".len()..];
        if rest.contains("Interval") {
            // Remove first Interval
            s = format!("{}{}", &s[..idx], &s[idx + "Interval".len()..]);
        } else {
            break;
        }
    }
    s = s.replace("SingleTileRegionsSingleTileRegion", "SingleTileRegion");
    s = s.replace("AcquisitionMode", ""); // already replaced Acquisition above
    s = s.replace("DetectorsDetector", "Detector");
    s = regex_replace(&s, "Setup[s]?", "");
    s = s.replace("Setting", "");
    s = s.replace("TrackTrack", "Track");
    s = s.replace("AnalogOutMaximumsAnalogOutMaximum", "AnalogOutMaximum");
    s = s.replace("AnalogOutMinimumsAnalogOutMinimum", "AnalogOutMinimum");
    s = s.replace(
        "DigitalOutLabelsDigitalOutLabelLabel",
        "DigitalOutLabelLabel",
    );
    s = s.replace("FocusDefiniteFocus", "FocusDefinite");
    s = s.replace("ChangerChanger", "Changer");
    s = s.replace("Calibration", "Cal");
    s = s.replace("LightSwitchChangerRLTLSwitch", "LightSwitchChangerRLTL");
    s = s.replace("Parameters", "");
    s = s.replace("Fluorescence", "Fluor");
    s = s.replace("CameraGeometryCameraGeometry", "CameraGeometry");
    s = s.replace("CameraCamera", "Camera");
    s = s.replace("DetectorsCamera", "Camera");
    s = s.replace(
        "FilterChangerLeftChangerEmissionFilter",
        "LeftChangerEmissionFilter",
    );
    s = s.replace("SwitchingStatesSwitchingState", "SwitchingState");
    s = s.replace("Information", "Info");
    // s/SubDimensions?//g
    s = s.replace("SubDimensions", "");
    s = s.replace("SubDimension", "");
    // s/Setups?//
    s = regex_replace_first(&s, "Setups?", "");
    // s/Parameters?//
    s = regex_replace_first(&s, "Parameters?", "");
    s = s.replace("Calculate", "Calc");
    s = s.replace("Visibility", "Vis");
    s = s.replace("Orientation", "Orient");
    s = s.replace("ListItems", "Items");
    s = s.replace("Increment", "Incr");
    s = s.replace("Parameter", "Param");
    // s/(ParfocalParcentralValues)+ParfocalParcentralValue/Parcentral/
    s = regex_replace(
        &s,
        "(ParfocalParcentralValues?)+ParfocalParcentralValues?",
        "Parcentral",
    );
    s = s.replace("ParcentralParcentral", "Parcentral");
    s = s.replace("CorrFocusCorrection", "FocusCorr");
    // s/(ApoTomeDepthInfo)+Element/ApoTomeDepth/
    s = regex_replace(&s, "(ApoTomeDepthInfo)+Element", "ApoTomeDepth");
    s = regex_replace(&s, "(ApoTomeClickStopInfo)+Element", "ApoTomeClickStop");
    s = s.replace("DepthDepth", "Depth");
    // s/(Devices?)+Device/Device/
    s = regex_replace(&s, "(Devices?)+Device", "Device");
    // s/(BeamPathNode)+/BeamPathNode/
    s = regex_replace(&s, "(BeamPathNode)+", "BeamPathNode");
    s = s.replace("BeamPathsBeamPath", "BeamPath");
    s = s.replace("BeamPathBeamPath", "BeamPath");
    s = s.replace("Configuration", "Config");
    s = s.replace("StageAxesStageAxis", "StageAxis");
    s = s.replace("RangesRange", "Range");
    s = s.replace("DataGridDatasGridData", "DataGrid");
    s = s.replace("DataMicroscopeDatasMicroscopeData", "DataMicroscope");
    s = s.replace("DataWegaDatasWegaData", "DataWega");
    s = s.replace("ClickStopPositionsClickStopPosition", "ClickStopPosition");
    // s/LightSourcess?LightSource(Settings)?(LightSource)?/LightSource/
    s = regex_replace(
        &s,
        "LightSourcess?LightSource(Settings)?(LightSource)?",
        "LightSource",
    );
    s = s.replace("FilterSetsFilterSet", "FilterSet");
    s = s.replace("EmissionFiltersEmissionFilter", "EmissionFilter");
    s = s.replace("ExcitationFiltersExcitationFilter", "ExcitationFilter");
    s = s.replace("FiltersFilter", "Filter");
    s = s.replace("DichroicsDichroic", "Dichronic");
    s = s.replace("WavelengthsWavelength", "Wavelength");
    s = s.replace("MultiTrackSetup", "MultiTrack");
    s = s.replace("TrackTrack", "Track");
    s = s.replace("DataGrabberSetup", "DataGrabber");
    s = s.replace("CameraFrameSetup", "CameraFrame");
    s = regex_replace(&s, "TimeSeries(TimeSeries|Setups)", "TimeSeries");
    s = s.replace("FocusFocus", "Focus");
    s = s.replace("FocusAutofocus", "Autofocus");
    // s/Focus(Hardware|Software)(Autofocus)+/Autofocus$1/
    s = regex_replace(&s, "Focus(Hardware|Software)(Autofocus)+", "Autofocus$1");
    s = s.replace("AutofocusAutofocus", "Autofocus");

    s
}

/// Simple regex replace (first occurrence only for non-global patterns).
fn regex_replace(s: &str, pat: &str, replacement: &str) -> String {
    // For simple patterns, use manual string matching
    // For patterns with anchors or groups, implement manually
    if let Some(pat_body) = pat.strip_prefix('^') {
        if let Some(stripped) = s.strip_prefix(pat_body) {
            return format!("{}{}", replacement, stripped);
        }
        return s.to_string();
    }
    // Non-anchored: find first occurrence
    // Handle simple capturing groups for replacement
    if pat.contains('(') {
        // Handle specific known patterns
        return czi_regex_replace_group(s, pat, replacement);
    }
    if let Some(idx) = s.find(pat) {
        format!("{}{}{}", &s[..idx], replacement, &s[idx + pat.len()..])
    } else {
        s.to_string()
    }
}

fn regex_replace_first(s: &str, pat: &str, replacement: &str) -> String {
    // Handle patterns like "Setups?" (optional s) and "Parameters?"
    let variants: Vec<&str> = if let Some(base) = pat.strip_suffix('?') {
        let long = pat.trim_end_matches('?');
        // "Setups?" → try "Setups" then "Setup"
        // We need both variants
        vec![long, base]
    } else {
        vec![pat]
    };
    // Try with the longer variant first
    if pat.ends_with('?') {
        let long = pat.trim_end_matches('?'); // "Setups" from "Setups?"
        let base = &long[..long.len() - 1]; // "Setup"
                                            // Try "Setups" first
        if let Some(idx) = s.find(long) {
            return format!("{}{}{}", &s[..idx], replacement, &s[idx + long.len()..]);
        }
        // Then try "Setup"
        if let Some(idx) = s.find(base) {
            return format!("{}{}{}", &s[..idx], replacement, &s[idx + base.len()..]);
        }
    }
    let _ = variants;
    s.to_string()
}

/// Handle regex replace for patterns with capturing groups.
fn czi_regex_replace_group(s: &str, pat: &str, _replacement: &str) -> String {
    // Handle specific known patterns:
    // "(Devices?)+Device" → "Device"
    // "(BeamPathNode)+" → "BeamPathNode"
    // etc.
    // For simplicity, handle specific patterns
    if pat == "(Devices?)+Device" {
        // Match one or more of "Device" or "Devices" followed by "Device"
        // Replace with "Device"
        // Pattern: DevicesDevice, DeviceDevicesDevice, etc.
        let result = s.to_string();
        // Iteratively replace DevicesDevice and DeviceDevice
        let mut r = result.clone();
        loop {
            let prev = r.clone();
            r = r.replace("DevicesDevice", "Device");
            r = r.replace("DeviceDevice", "Device");
            if r == prev {
                break;
            }
        }
        return r;
    }
    if pat == "(BeamPathNode)+" {
        // Replace multiple BeamPathNode with single
        let mut r = s.to_string();
        loop {
            let prev = r.clone();
            r = r.replace("BeamPathNodeBeamPathNode", "BeamPathNode");
            if r == prev {
                break;
            }
        }
        return r;
    }
    if pat == "ManagerContrastManager(Contrast)?" {
        // Replace "ManagerContrastManagerContrast" or "ManagerContrastManager" with "ManagerContrast"
        let r = s.replace("ManagerContrastManagerContrast", "ManagerContrast");
        let r = r.replace("ManagerContrastManager", "ManagerContrast");
        return r;
    }
    if pat.starts_with("Focus(Hardware|Software)(Autofocus)+") {
        // s/Focus(Hardware|Software)(Autofocus)+/Autofocus$1/
        for suffix in &["Hardware", "Software"] {
            let search = format!("Focus{}Autofocus", suffix);
            if let Some(idx) = s.find(&search) {
                // Replace Focus{X}(Autofocus)+ with Autofocus{X}
                // First consume all Autofocus repetitions
                let mut end = idx + search.len();
                while s[end..].starts_with("Autofocus") {
                    end += "Autofocus".len();
                }
                return format!("{}Autofocus{}{}", &s[..idx], suffix, &s[end..]);
            }
        }
        return s.to_string();
    }
    if pat.starts_with("LightSourcess?LightSource") {
        let r = s.replace("LightSourcessLightSourceSettingsLightSource", "LightSource");
        let r = r.replace("LightSourcessLightSourceSettings", "LightSource");
        let r = r.replace("LightSourcessLightSourceLightSource", "LightSource");
        let r = r.replace("LightSourcessLightSource", "LightSource");
        let r = r.replace("LightSourcesLightSourceSettingsLightSource", "LightSource");
        let r = r.replace("LightSourcesLightSourceSettings", "LightSource");
        let r = r.replace("LightSourcesLightSourceLightSource", "LightSource");
        let r = r.replace("LightSourcesLightSource", "LightSource");
        return r;
    }
    if pat.starts_with("TimeSeries(TimeSeries|Setups)") {
        let r = s.replace("TimeSeriesTimeSeries", "TimeSeries");
        let r = r.replace("TimeSeriesSetups", "TimeSeries");
        return r;
    }
    if pat.starts_with("(ApoTomeDepthInfo)+Element") {
        let mut r = s.to_string();
        loop {
            let prev = r.clone();
            r = r.replace(
                "ApoTomeDepthInfoApoTomeDepthInfoElement",
                "ApoTomeDepthInfoElement",
            );
            if r == prev {
                break;
            }
        }
        r = r.replace("ApoTomeDepthInfoElement", "ApoTomeDepth");
        return r;
    }
    if pat.starts_with("(ApoTomeClickStopInfo)+Element") {
        let mut r = s.to_string();
        r = r.replace("ApoTomeClickStopInfoElement", "ApoTomeClickStop");
        return r;
    }
    if pat.starts_with("(ParfocalParcentralValues?)+") {
        let r = s.replace(
            "ParfocalParcentralValuesParfocalParcentralValue",
            "Parcentral",
        );
        let r = r.replace(
            "ParfocalParcentralValueParfocalParcentralValue",
            "Parcentral",
        );
        let r = r.replace("ParfocalParcentralValue", "Parcentral");
        return r;
    }
    // Default: no-op for unhandled patterns
    s.to_string()
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
