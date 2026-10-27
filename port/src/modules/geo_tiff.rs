//! Image::ExifTool::GeoTiff (Image/ExifTool/GeoTiff.pm), ported sub by sub (see docs/port/PORTING.md).

use crate::interp::eval::ER;
use crate::interp::{Interp, Want};
use crate::modules::exiftool::part2::get16u;
use crate::perl::Value;
use crate::pl::{self, arg, n, re};

const MODULE: &str = "Image::ExifTool::GeoTiff";

fn g16u(it: &mut Interp, data_pt: &Value, pos: Value) -> ER<Value> {
    Ok(get16u(it, vec![data_pt.clone(), pos], Want::Scalar)?.pop().unwrap_or_default())
}

/// Inputs: 0) ExifTool object reference
/// Notes: byte order must be set before calling this routine
pub fn process_geo_tiff(it: &mut Interp, a: Vec<Value>, want: Want) -> ER<Vec<Value>> {
    let et = arg(&a, 0);
    let dir_data = pl::method(it, &et, "GetValue", vec![pl::s("GeoTiffDirectory"), pl::s("ValueConv")])?;
    if !dir_data.truthy() {
        return pl::ret_nothing(want);
    }

    // avoid re-processing if another EXIF directory is found
    let did = et.get("DidGeoTiff");
    if did.truthy() && did.to_bytes() == dir_data.to_bytes() {
        return pl::ret_nothing(want);
    }
    et.set("DidGeoTiff", dir_data.clone());

    let double_data = pl::method(it, &et, "GetValue", vec![pl::s("GeoTiffDoubleParams"), pl::s("ValueConv")])?;
    let ascii_data = pl::method(it, &et, "GetValue", vec![pl::s("GeoTiffAsciiParams"), pl::s("ValueConv")])?;
    let verbose = pl::method(it, &et, "Options", vec![pl::s("Verbose")])?;

    pl::at(2146);
    let dir_len = pl::length(&pl::sderef(&dir_data)?);
    let ok = pl::ge(&dir_len, &n(8)) && {
        let ne = g16u(it, &dir_data, n(6))?;
        pl::ge(&dir_len, &pl::mul(&n(8), &pl::add(&ne, &n(1))))
    };
    if ok {
        let version = g16u(it, &dir_data, n(0))?;
        let revision = g16u(it, &dir_data, n(2))?;
        let minor_rev = g16u(it, &dir_data, n(4))?;
        let num_entries = g16u(it, &dir_data, n(6))?;

        if verbose.truthy() {
            et.set("INDENT", pl::concat(&et.get("INDENT"), &pl::s("| ")));
            pl::method(it, &et, "VerboseDir", vec![pl::s("GeoTiff"), num_entries.clone()])?;
        }
        // generate version number tag (not a real GeoTiff tag)
        let tag_table = pl::call(it, "Image::ExifTool::GetTagTable", vec![pl::s("Image::ExifTool::GeoTiff::Main")])?;
        let mut tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table.clone(), n(1)])?;
        if tag_info.truthy() {
            let v = pl::join(".", &[version, revision, minor_rev]);
            pl::method(it, &et, "FoundTag", vec![tag_info.clone(), v])?;
        }

        let geo_tiff_format = crate::runtime::lex_hash(it, MODULE, "geoTiffFormat");
        let mut i = n(0);
        while pl::lt(&i, &num_entries) {
            'entry: {
                let pt = pl::mul(&n(8), &pl::add(&i, &n(1)));
                let tag = g16u(it, &dir_data, pt.clone())?;
                tag_info = pl::method(it, &et, "GetTagInfo", vec![tag_table.clone(), tag.clone()])?;
                if !tag_info.truthy() {
                    break 'entry;
                }
                let loc = g16u(it, &dir_data, pl::add(&pt, &n(2)))?;
                let mut count = g16u(it, &dir_data, pl::add(&pt, &n(4)))?;
                let mut offset = g16u(it, &dir_data, pl::add(&pt, &n(6)))?;
                let format = geo_tiff_format.get(loc.to_bytes());
                let mut data_pt = Value::Undef;
                if !format.truthy() {
                    let msg = pl::concat(&pl::concat(&pl::s("Unknown GeoTiff location ("), &loc), &pl::concat(&pl::s(") for "), &tag_info.get("Name")));
                    pl::method(it, &et, "Warn", vec![msg])?;
                    break 'entry;
                } else if pl::str_eq(&format, "double") {
                    // in the double parms
                    data_pt = double_data.clone();
                } else if pl::str_eq(&format, "string") {
                    // in the ASCII parms
                    data_pt = ascii_data.clone();
                } else if pl::str_eq(&format, "int16u") {
                    // in the GeoTiffDirectory data
                    data_pt = dir_data.clone();
                    if !loc.truthy() {
                        // is value is stored in offset?
                        count = n(1); // (implied by location of 0)
                        offset = pl::div(&pl::add(&pt, &n(6)), &n(2))?; // offset of the "offset" value
                    }
                }
                let size = pl::call(it, "Image::ExifTool::FormatSize", vec![format.clone()])?;
                pl::at(2188);
                if !data_pt.truthy() || pl::lt(&pl::length(&pl::sderef(&data_pt)?), &pl::mul(&size, &pl::add(&offset, &count))) {
                    let msg = pl::concat(&pl::concat(&pl::s("Missing "), &format), &pl::concat(&pl::s(" data for "), &tag_info.get("Name")));
                    pl::method(it, &et, "Warn", vec![msg])?;
                    break 'entry;
                }
                offset = pl::mul(&offset, &size);
                let rem = pl::sub(&pl::length(&data_pt.deref()), &offset);
                let mut val = pl::call(it, "Image::ExifTool::ReadValue", vec![data_pt.clone(), offset.clone(), format.clone(), count.clone(), rem])?;
                // remove trailing terminator (NULL or '|') from string value
                if pl::str_eq(&format, "string") {
                    re(r"(\0|\|)$", "").subst(&mut val, false, |_| Vec::new());
                }
                if verbose.truthy() {
                    pl::method(
                        it,
                        &et,
                        "VerboseInfo",
                        vec![
                            tag.clone(),
                            tag_info.clone(),
                            pl::s("Table"),
                            tag_table.clone(),
                            pl::s("Index"),
                            i.clone(),
                            pl::s("Value"),
                            val.clone(),
                            pl::s("DataPt"),
                            data_pt.clone(),
                            pl::s("Start"),
                            offset.clone(),
                            pl::s("Format"),
                            format.clone(),
                            pl::s("Count"),
                            count.clone(),
                            pl::s("Size"),
                            pl::mul(&count, &size),
                        ],
                    )?;
                }
                pl::method(it, &et, "FoundTag", vec![tag_info.clone(), val])?;
            }
            i = pl::add(&i, &n(1));
        }
        if verbose.truthy() {
            let mut indent = et.get("INDENT");
            re(r"..$", "").subst(&mut indent, false, |_| Vec::new());
            et.set("INDENT", indent);
        }
    } else {
        pl::method(it, &et, "Warn", vec![pl::s("Bad GeoTIFF directory")])?;
    }
    // extract block tags only if requested
    if !et.get_or_new_hash("OPTIONS").get("RequestAll").truthy() {
        for (lk, name) in [("geotiffdirectory", "GeoTiffDirectory"), ("geotiffdoubleparams", "GeoTiffDoubleParams"), ("geotiffasciiparams", "GeoTiffAsciiParams")] {
            if !et.get_or_new_hash("REQ_TAG_LOOKUP").get(lk).truthy() {
                pl::method(it, &et, "DeleteTag", vec![pl::s(name)])?;
            }
        }
    }
    pl::ret_nothing(want)
}

/// Register the subs ported so far; the others still run from the Perl.
pub fn register(it: &mut Interp) {
    it.define_native("Image::ExifTool::GeoTiff::ProcessGeoTiff", process_geo_tiff);
}
