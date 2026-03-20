//! DICOM (Digital Imaging and Communications in Medicine) format reader.
//! Mirrors Image::ExifTool::DICOM - reads DICOM and ACR-NEMA medical images.

use crate::error::{Error, Result};
use crate::tag::{Tag, TagGroup, TagId};
use crate::value::Value;

fn mktag(name: &str, desc: &str, value: Value) -> Tag {
    let pv = value.to_display_string();
    Tag {
        id: TagId::Text(name.to_string()),
        name: name.to_string(),
        description: desc.to_string(),
        group: TagGroup {
            family0: "DICOM".into(),
            family1: "DICOM".into(),
            family2: "Image".into(),
        },
        raw_value: value,
        print_value: pv,
        priority: 0,
    }
}

// VR types that use 32-bit length fields in explicit VR syntax
fn is_vr32(vr: &[u8]) -> bool {
    matches!(vr, b"OB" | b"OW" | b"OF" | b"SQ" | b"UT" | b"UN")
}

// Tags that are always implicit VR regardless of syntax
fn is_implicit_tag(group: u16, element: u16) -> bool {
    matches!((group, element),
        (0xFFFE, 0xE000) | (0xFFFE, 0xE00D) | (0xFFFE, 0xE0DD))
}

/// Look up tag name for a (group, element) pair.
/// Returns (name, description) if known.
fn lookup_tag(group: u16, element: u16) -> Option<(&'static str, &'static str)> {
    match (group, element) {
        // File meta information group (0002)
        (0x0002, 0x0000) => Some(("FileMetaInfoGroupLength", "File Meta Info Group Length")),
        (0x0002, 0x0001) => Some(("FileMetaInfoVersion", "File Meta Info Version")),
        (0x0002, 0x0002) => Some(("MediaStorageSOPClassUID", "Media Storage SOP Class UID")),
        (0x0002, 0x0003) => Some(("MediaStorageSOPInstanceUID", "Media Storage SOP Instance UID")),
        (0x0002, 0x0010) => Some(("TransferSyntaxUID", "Transfer Syntax UID")),
        (0x0002, 0x0012) => Some(("ImplementationClassUID", "Implementation Class UID")),
        (0x0002, 0x0013) => Some(("ImplementationVersionName", "Implementation Version Name")),
        (0x0002, 0x0016) => Some(("SourceApplicationEntityTitle", "Source Application Entity Title")),
        (0x0002, 0x0100) => Some(("PrivateInformationCreatorUID", "Private Information Creator UID")),
        (0x0002, 0x0102) => Some(("PrivateInformation", "Private Information")),
        // Identifying group (0008)
        (0x0008, 0x0000) => Some(("IdentifyingGroupLength", "Identifying Group Length")),
        (0x0008, 0x0005) => Some(("SpecificCharacterSet", "Specific Character Set")),
        (0x0008, 0x0008) => Some(("ImageType", "Image Type")),
        (0x0008, 0x0010) => Some(("RecognitionCode", "Recognition Code")),
        (0x0008, 0x0012) => Some(("InstanceCreationDate", "Instance Creation Date")),
        (0x0008, 0x0013) => Some(("InstanceCreationTime", "Instance Creation Time")),
        (0x0008, 0x0016) => Some(("SOPClassUID", "SOP Class UID")),
        (0x0008, 0x0018) => Some(("SOPInstanceUID", "SOP Instance UID")),
        (0x0008, 0x0020) => Some(("StudyDate", "Study Date")),
        (0x0008, 0x0021) => Some(("SeriesDate", "Series Date")),
        (0x0008, 0x0022) => Some(("AcquisitionDate", "Acquisition Date")),
        (0x0008, 0x0023) => Some(("ContentDate", "Content Date")),
        (0x0008, 0x0024) => Some(("OverlayDate", "Overlay Date")),
        (0x0008, 0x0025) => Some(("CurveDate", "Curve Date")),
        (0x0008, 0x002A) => Some(("AcquisitionDateTime", "Acquisition Date Time")),
        (0x0008, 0x0030) => Some(("StudyTime", "Study Time")),
        (0x0008, 0x0031) => Some(("SeriesTime", "Series Time")),
        (0x0008, 0x0032) => Some(("AcquisitionTime", "Acquisition Time")),
        (0x0008, 0x0033) => Some(("ContentTime", "Content Time")),
        (0x0008, 0x0034) => Some(("OverlayTime", "Overlay Time")),
        (0x0008, 0x0035) => Some(("CurveTime", "Curve Time")),
        (0x0008, 0x0040) => Some(("DataSetType", "Data Set Type")),
        (0x0008, 0x0041) => Some(("DataSetSubtype", "Data Set Subtype")),
        (0x0008, 0x0050) => Some(("AccessionNumber", "Accession Number")),
        (0x0008, 0x0060) => Some(("Modality", "Modality")),
        (0x0008, 0x0061) => Some(("ModalitiesInStudy", "Modalities In Study")),
        (0x0008, 0x0064) => Some(("ConversionType", "Conversion Type")),
        (0x0008, 0x0068) => Some(("PresentationIntentType", "Presentation Intent Type")),
        (0x0008, 0x0070) => Some(("Manufacturer", "Manufacturer")),
        (0x0008, 0x0080) => Some(("InstitutionName", "Institution Name")),
        (0x0008, 0x0081) => Some(("InstitutionAddress", "Institution Address")),
        (0x0008, 0x0090) => Some(("ReferringPhysicianName", "Referring Physician Name")),
        (0x0008, 0x0092) => Some(("ReferringPhysicianAddress", "Referring Physician Address")),
        (0x0008, 0x0094) => Some(("ReferringPhysicianTelephoneNumber", "Referring Physician Telephone Number")),
        (0x0008, 0x0100) => Some(("CodeValue", "Code Value")),
        (0x0008, 0x0102) => Some(("CodingSchemeDesignator", "Coding Scheme Designator")),
        (0x0008, 0x0103) => Some(("CodingSchemeVersion", "Coding Scheme Version")),
        (0x0008, 0x0104) => Some(("CodeMeaning", "Code Meaning")),
        (0x0008, 0x0201) => Some(("TimezoneOffsetFromUTC", "Timezone Offset From UTC")),
        (0x0008, 0x1000) => Some(("NetworkID", "Network ID")),
        (0x0008, 0x1010) => Some(("StationName", "Station Name")),
        (0x0008, 0x1030) => Some(("StudyDescription", "Study Description")),
        (0x0008, 0x103E) => Some(("SeriesDescription", "Series Description")),
        (0x0008, 0x1040) => Some(("InstitutionalDepartmentName", "Institutional Department Name")),
        (0x0008, 0x1048) => Some(("PhysiciansOfRecord", "Physicians Of Record")),
        (0x0008, 0x1050) => Some(("PerformingPhysicianName", "Performing Physician Name")),
        (0x0008, 0x1060) => Some(("NameOfPhysicianReadingStudy", "Name Of Physician Reading Study")),
        (0x0008, 0x1070) => Some(("OperatorsName", "Operators Name")),
        (0x0008, 0x1080) => Some(("AdmittingDiagnosesDescription", "Admitting Diagnoses Description")),
        (0x0008, 0x1090) => Some(("ManufacturersModelName", "Manufacturers Model Name")),
        (0x0008, 0x1150) => Some(("ReferencedSOPClassUID", "Referenced SOP Class UID")),
        (0x0008, 0x1155) => Some(("ReferencedSOPInstanceUID", "Referenced SOP Instance UID")),
        (0x0008, 0x2110) => Some(("LossyImageCompression", "Lossy Image Compression")),
        (0x0008, 0x2111) => Some(("DerivationDescription", "Derivation Description")),
        (0x0008, 0x4000) => Some(("IdentifyingComments", "Identifying Comments")),
        // Patient group (0010)
        (0x0010, 0x0000) => Some(("PatientGroupLength", "Patient Group Length")),
        (0x0010, 0x0010) => Some(("PatientName", "Patient Name")),
        (0x0010, 0x0020) => Some(("PatientID", "Patient ID")),
        (0x0010, 0x0021) => Some(("IssuerOfPatientID", "Issuer Of Patient ID")),
        (0x0010, 0x0030) => Some(("PatientBirthDate", "Patient Birth Date")),
        (0x0010, 0x0032) => Some(("PatientBirthTime", "Patient Birth Time")),
        (0x0010, 0x0040) => Some(("PatientSex", "Patient Sex")),
        (0x0010, 0x1000) => Some(("OtherPatientIDs", "Other Patient IDs")),
        (0x0010, 0x1001) => Some(("OtherPatientNames", "Other Patient Names")),
        (0x0010, 0x1005) => Some(("PatientBirthName", "Patient Birth Name")),
        (0x0010, 0x1010) => Some(("PatientAge", "Patient Age")),
        (0x0010, 0x1020) => Some(("PatientSize", "Patient Size")),
        (0x0010, 0x1030) => Some(("PatientWeight", "Patient Weight")),
        (0x0010, 0x1040) => Some(("PatientAddress", "Patient Address")),
        (0x0010, 0x1060) => Some(("PatientMotherBirthName", "Patient Mother Birth Name")),
        (0x0010, 0x1080) => Some(("MilitaryRank", "Military Rank")),
        (0x0010, 0x1090) => Some(("MedicalRecordLocator", "Medical Record Locator")),
        (0x0010, 0x2000) => Some(("MedicalAlerts", "Medical Alerts")),
        (0x0010, 0x2110) => Some(("Allergies", "Allergies")),
        (0x0010, 0x2150) => Some(("CountryOfResidence", "Country Of Residence")),
        (0x0010, 0x2154) => Some(("PatientTelephoneNumbers", "Patient Telephone Numbers")),
        (0x0010, 0x2160) => Some(("EthnicGroup", "Ethnic Group")),
        (0x0010, 0x2180) => Some(("Occupation", "Occupation")),
        (0x0010, 0x21B0) => Some(("AdditionalPatientHistory", "Additional Patient History")),
        (0x0010, 0x4000) => Some(("PatientComments", "Patient Comments")),
        // Acquisition group (0018)
        (0x0018, 0x0000) => Some(("AcquisitionGroupLength", "Acquisition Group Length")),
        (0x0018, 0x0010) => Some(("ContrastBolusAgent", "Contrast Bolus Agent")),
        (0x0018, 0x0015) => Some(("BodyPartExamined", "Body Part Examined")),
        (0x0018, 0x0020) => Some(("ScanningSequence", "Scanning Sequence")),
        (0x0018, 0x0021) => Some(("SequenceVariant", "Sequence Variant")),
        (0x0018, 0x0022) => Some(("ScanOptions", "Scan Options")),
        (0x0018, 0x0023) => Some(("MRAcquisitionType", "MR Acquisition Type")),
        (0x0018, 0x0024) => Some(("SequenceName", "Sequence Name")),
        (0x0018, 0x0025) => Some(("AngioFlag", "Angio Flag")),
        (0x0018, 0x0040) => Some(("CineRate", "Cine Rate")),
        (0x0018, 0x0050) => Some(("SliceThickness", "Slice Thickness")),
        (0x0018, 0x0060) => Some(("KVP", "KVP")),
        (0x0018, 0x0070) => Some(("CountsAccumulated", "Counts Accumulated")),
        (0x0018, 0x0080) => Some(("RepetitionTime", "Repetition Time")),
        (0x0018, 0x0081) => Some(("EchoTime", "Echo Time")),
        (0x0018, 0x0082) => Some(("InversionTime", "Inversion Time")),
        (0x0018, 0x0083) => Some(("NumberOfAverages", "Number Of Averages")),
        (0x0018, 0x0084) => Some(("ImagingFrequency", "Imaging Frequency")),
        (0x0018, 0x0085) => Some(("ImagedNucleus", "Imaged Nucleus")),
        (0x0018, 0x0086) => Some(("EchoNumber", "Echo Number")),
        (0x0018, 0x0087) => Some(("MagneticFieldStrength", "Magnetic Field Strength")),
        (0x0018, 0x0088) => Some(("SpacingBetweenSlices", "Spacing Between Slices")),
        (0x0018, 0x0089) => Some(("NumberOfPhaseEncodingSteps", "Number Of Phase Encoding Steps")),
        (0x0018, 0x0090) => Some(("DataCollectionDiameter", "Data Collection Diameter")),
        (0x0018, 0x0091) => Some(("EchoTrainLength", "Echo Train Length")),
        (0x0018, 0x0093) => Some(("PercentSampling", "Percent Sampling")),
        (0x0018, 0x0094) => Some(("PercentPhaseFieldOfView", "Percent Phase Field Of View")),
        (0x0018, 0x0095) => Some(("PixelBandwidth", "Pixel Bandwidth")),
        (0x0018, 0x1000) => Some(("DeviceSerialNumber", "Device Serial Number")),
        (0x0018, 0x1004) => Some(("PlateID", "Plate ID")),
        (0x0018, 0x1010) => Some(("SecondaryCaptureDeviceID", "Secondary Capture Device ID")),
        (0x0018, 0x1016) => Some(("SecondaryCaptureDeviceManufacturer", "Secondary Capture Device Manufacturer")),
        (0x0018, 0x1018) => Some(("SecondaryCaptureDeviceManufacturersModelName", "Secondary Capture Device Manufacturers Model Name")),
        (0x0018, 0x1019) => Some(("SecondaryCaptureDeviceSoftwareVersion", "Secondary Capture Device Software Version")),
        (0x0018, 0x1020) => Some(("SoftwareVersion", "Software Version")),
        (0x0018, 0x1030) => Some(("ProtocolName", "Protocol Name")),
        (0x0018, 0x1040) => Some(("ContrastBolusRoute", "Contrast Bolus Route")),
        (0x0018, 0x1050) => Some(("SpatialResolution", "Spatial Resolution")),
        (0x0018, 0x1060) => Some(("TriggerTime", "Trigger Time")),
        (0x0018, 0x1062) => Some(("NominalInterval", "Nominal Interval")),
        (0x0018, 0x1063) => Some(("FrameTime", "Frame Time")),
        (0x0018, 0x1065) => Some(("FrameTimeVector", "Frame Time Vector")),
        (0x0018, 0x1066) => Some(("FrameDelay", "Frame Delay")),
        (0x0018, 0x1088) => Some(("HeartRate", "Heart Rate")),
        (0x0018, 0x1090) => Some(("CardiacNumberOfImages", "Cardiac Number Of Images")),
        (0x0018, 0x1094) => Some(("TriggerWindow", "Trigger Window")),
        (0x0018, 0x1100) => Some(("ReconstructionDiameter", "Reconstruction Diameter")),
        (0x0018, 0x1164) => Some(("ImagerPixelSpacing", "Imager Pixel Spacing")),
        (0x0018, 0x1166) => Some(("Grid", "Grid")),
        (0x0018, 0x1170) => Some(("GeneratorPower", "Generator Power")),
        (0x0018, 0x1190) => Some(("FocalSpot", "Focal Spot")),
        (0x0018, 0x1200) => Some(("DateOfLastCalibration", "Date Of Last Calibration")),
        (0x0018, 0x1201) => Some(("TimeOfLastCalibration", "Time Of Last Calibration")),
        (0x0018, 0x1250) => Some(("ReceiveCoilName", "Receive Coil Name")),
        (0x0018, 0x1251) => Some(("TransmitCoilName", "Transmit Coil Name")),
        (0x0018, 0x1310) => Some(("AcquisitionMatrix", "Acquisition Matrix")),
        (0x0018, 0x1312) => Some(("InPlanePhaseEncodingDirection", "In Plane Phase Encoding Direction")),
        (0x0018, 0x1314) => Some(("FlipAngle", "Flip Angle")),
        (0x0018, 0x1315) => Some(("VariableFlipAngleFlag", "Variable Flip Angle Flag")),
        (0x0018, 0x1316) => Some(("SAR", "SAR")),
        (0x0018, 0x1318) => Some(("dBdt", "dBdt")),
        (0x0018, 0x5100) => Some(("PatientPosition", "Patient Position")),
        (0x0018, 0x5101) => Some(("ViewPosition", "View Position")),
        // Relationship group (0020)
        (0x0020, 0x000D) => Some(("StudyInstanceUID", "Study Instance UID")),
        (0x0020, 0x000E) => Some(("SeriesInstanceUID", "Series Instance UID")),
        (0x0020, 0x0010) => Some(("StudyID", "Study ID")),
        (0x0020, 0x0011) => Some(("SeriesNumber", "Series Number")),
        (0x0020, 0x0012) => Some(("AcquisitionNumber", "Acquisition Number")),
        (0x0020, 0x0013) => Some(("InstanceNumber", "Instance Number")),
        (0x0020, 0x0020) => Some(("PatientOrientation", "Patient Orientation")),
        (0x0020, 0x0030) => Some(("ImagePosition", "Image Position")),
        (0x0020, 0x0032) => Some(("ImagePositionPatient", "Image Position Patient")),
        (0x0020, 0x0035) => Some(("ImageOrientation", "Image Orientation")),
        (0x0020, 0x0037) => Some(("ImageOrientationPatient", "Image Orientation Patient")),
        (0x0020, 0x0052) => Some(("FrameOfReferenceUID", "Frame Of Reference UID")),
        (0x0020, 0x0060) => Some(("Laterality", "Laterality")),
        (0x0020, 0x0100) => Some(("TemporalPositionIdentifier", "Temporal Position Identifier")),
        (0x0020, 0x0105) => Some(("NumberOfTemporalPositions", "Number Of Temporal Positions")),
        (0x0020, 0x0110) => Some(("TemporalResolution", "Temporal Resolution")),
        (0x0020, 0x1000) => Some(("SeriesInStudy", "Series In Study")),
        (0x0020, 0x1002) => Some(("ImagesInAcquisition", "Images In Acquisition")),
        (0x0020, 0x1004) => Some(("AcquisitionsInStudy", "Acquisitions In Study")),
        (0x0020, 0x1040) => Some(("PositionReferenceIndicator", "Position Reference Indicator")),
        (0x0020, 0x1041) => Some(("SliceLocation", "Slice Location")),
        (0x0020, 0x3401) => Some(("ModifyingDeviceID", "Modifying Device ID")),
        (0x0020, 0x3402) => Some(("ModifiedImageID", "Modified Image ID")),
        (0x0020, 0x3404) => Some(("ModifyingDeviceManufacturer", "Modifying Device Manufacturer")),
        (0x0020, 0x3406) => Some(("ModifiedImageDescription", "Modified Image Description")),
        (0x0020, 0x4000) => Some(("ImageComments", "Image Comments")),
        // Image group (0028)
        (0x0028, 0x0002) => Some(("SamplesPerPixel", "Samples Per Pixel")),
        (0x0028, 0x0004) => Some(("PhotometricInterpretation", "Photometric Interpretation")),
        (0x0028, 0x0006) => Some(("PlanarConfiguration", "Planar Configuration")),
        (0x0028, 0x0008) => Some(("NumberOfFrames", "Number Of Frames")),
        (0x0028, 0x0009) => Some(("FrameIncrementPointer", "Frame Increment Pointer")),
        (0x0028, 0x0010) => Some(("Rows", "Rows")),
        (0x0028, 0x0011) => Some(("Columns", "Columns")),
        (0x0028, 0x0030) => Some(("PixelSpacing", "Pixel Spacing")),
        (0x0028, 0x0034) => Some(("PixelAspectRatio", "Pixel Aspect Ratio")),
        (0x0028, 0x0051) => Some(("CorrectedImage", "Corrected Image")),
        (0x0028, 0x0100) => Some(("BitsAllocated", "Bits Allocated")),
        (0x0028, 0x0101) => Some(("BitsStored", "Bits Stored")),
        (0x0028, 0x0102) => Some(("HighBit", "High Bit")),
        (0x0028, 0x0103) => Some(("PixelRepresentation", "Pixel Representation")),
        (0x0028, 0x0106) => Some(("SmallestImagePixelValue", "Smallest Image Pixel Value")),
        (0x0028, 0x0107) => Some(("LargestImagePixelValue", "Largest Image Pixel Value")),
        (0x0028, 0x0120) => Some(("PixelPaddingValue", "Pixel Padding Value")),
        (0x0028, 0x0300) => Some(("QualityControlImage", "Quality Control Image")),
        (0x0028, 0x0301) => Some(("BurnedInAnnotation", "Burned In Annotation")),
        (0x0028, 0x1050) => Some(("WindowCenter", "Window Center")),
        (0x0028, 0x1051) => Some(("WindowWidth", "Window Width")),
        (0x0028, 0x1052) => Some(("RescaleIntercept", "Rescale Intercept")),
        (0x0028, 0x1053) => Some(("RescaleSlope", "Rescale Slope")),
        (0x0028, 0x1054) => Some(("RescaleType", "Rescale Type")),
        (0x0028, 0x1055) => Some(("WindowCenterAndWidthExplanation", "Window Center And Width Explanation")),
        (0x0028, 0x1101) => Some(("RedPaletteColorLookupTableDescriptor", "Red Palette Color Lookup Table Descriptor")),
        (0x0028, 0x1102) => Some(("GreenPaletteColorLookupTableDescriptor", "Green Palette Color Lookup Table Descriptor")),
        (0x0028, 0x1103) => Some(("BluePaletteColorLookupTableDescriptor", "Blue Palette Color Lookup Table Descriptor")),
        // Pixel data (7FE0)
        (0x7FE0, 0x0010) => Some(("PixelData", "Pixel Data")),
        _ => None,
    }
}

/// UID lookup for print conversion of UI-type tags.
fn lookup_uid(uid: &str) -> Option<&'static str> {
    match uid {
        "1.2.840.10008.1.1" => Some("Verification SOP Class"),
        "1.2.840.10008.1.2" => Some("Implicit VR Little Endian"),
        "1.2.840.10008.1.2.1" => Some("Explicit VR Little Endian"),
        "1.2.840.10008.1.2.1.99" => Some("Deflated Explicit VR Little Endian"),
        "1.2.840.10008.1.2.2" => Some("Explicit VR Big Endian"),
        "1.2.840.10008.1.2.4.50" => Some("JPEG Baseline (Process 1)"),
        "1.2.840.10008.1.2.4.51" => Some("JPEG Extended (Process 2 & 4)"),
        "1.2.840.10008.1.2.4.57" => Some("JPEG Lossless, Non-Hierarchical (Process 14)"),
        "1.2.840.10008.1.2.4.70" => Some("JPEG Lossless, Non-Hierarchical, First-Order Prediction (Process 14-1)"),
        "1.2.840.10008.1.2.4.80" => Some("JPEG-LS Lossless Image Compression"),
        "1.2.840.10008.1.2.4.81" => Some("JPEG-LS Lossy (Near-Lossless) Image Compression"),
        "1.2.840.10008.1.2.4.90" => Some("JPEG 2000 Image Compression (Lossless Only)"),
        "1.2.840.10008.1.2.4.91" => Some("JPEG 2000 Image Compression"),
        "1.2.840.10008.1.2.5" => Some("RLE Lossless"),
        "1.2.840.10008.5.1.4.1.1.2" => Some("CT Image Storage"),
        "1.2.840.10008.5.1.4.1.1.4" => Some("MR Image Storage"),
        "1.2.840.10008.5.1.4.1.1.6.1" => Some("Ultrasound Image Storage"),
        "1.2.840.10008.5.1.4.1.1.7" => Some("Secondary Capture Image Storage"),
        "1.2.840.10008.5.1.4.1.1.12.1" => Some("X-Ray Angiographic Image Storage"),
        "1.2.840.10008.5.1.4.1.1.20" => Some("Nuclear Medicine Image Storage"),
        "1.2.840.10008.5.1.4.1.1.128" => Some("Positron Emission Tomography Image Storage"),
        "1.2.840.10008.5.1.4.1.1.481.1" => Some("RT Image Storage"),
        "1.2.840.10008.5.1.4.1.1.481.2" => Some("RT Dose Storage"),
        "1.2.840.10008.5.1.4.1.1.481.3" => Some("RT Structure Set Storage"),
        "1.2.840.10008.5.1.4.1.1.481.5" => Some("RT Plan Storage"),
        _ => None,
    }
}

/// Format a DICOM date value (YYYYMMDD -> YYYY:MM:DD)
fn format_date(s: &str) -> String {
    let s = s.trim();
    if s.len() == 8 && s.chars().all(|c| c.is_ascii_digit()) {
        format!("{}:{}:{}", &s[0..4], &s[4..6], &s[6..8])
    } else {
        s.to_string()
    }
}

/// Format a DICOM time value (HHMMSS... -> HH:MM:SS...)
fn format_time(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 6 && s[0..6].chars().all(|c| c.is_ascii_digit()) {
        format!("{}:{}:{}", &s[0..2], &s[2..4], &s[4..])
    } else {
        s.to_string()
    }
}

pub fn read_dicom(data: &[u8]) -> Result<Vec<Tag>> {
    if data.len() < 132 || &data[128..132] != b"DICM" {
        return Err(Error::InvalidData("not a DICOM file".into()));
    }

    let mut tags = Vec::new();
    let mut pos = 132usize;
    let mut implicit = false;
    let mut transfer_syntax: Option<String> = None;
    let mut group2end: Option<usize> = None;
    let mut big_endian = false;

    while pos + 8 <= data.len() {
        // Check if we need to apply transfer syntax after group 2
        if let Some(ts) = transfer_syntax.take() {
            // Only process transfer syntax when we've left group 2
            let grp = read_u16(data, pos, big_endian);
            if grp != 0x0002 || group2end.map_or(false, |end| pos >= end) {
                if ts.starts_with("1.2.840.10008.1.2.") || ts.starts_with("1.2.840.10008.1.2") {
                    // Check for implicit VR
                    if ts == "1.2.840.10008.1.2" {
                        implicit = true;
                    } else if ts.contains(".2") && ts == "1.2.840.10008.1.2.2" {
                        big_endian = true;
                    }
                }
                // transfer_syntax already consumed, don't put it back
            } else {
                // Still in group 2, restore
                transfer_syntax = Some(ts);
            }
        }

        // Read group and element
        let group = read_u16(data, pos, big_endian);
        let element = read_u16(data, pos + 2, big_endian);
        let tag_implicit = implicit || is_implicit_tag(group, element);

        let (vr, val_len, hdr_size) = if tag_implicit {
            let len = read_u32(data, pos + 4, big_endian) as usize;
            (b"  " as &[u8], len, 8usize)
        } else {
            let vr = &data[pos + 4..pos + 6];
            // Must be ASCII uppercase
            if !vr[0].is_ascii_uppercase() || !vr[1].is_ascii_uppercase() {
                // Fall back to implicit
                let len = read_u32(data, pos + 4, big_endian) as usize;
                (b"  " as &[u8], len, 8usize)
            } else if is_vr32(vr) {
                // 2 reserved bytes then 4-byte length
                if pos + 12 > data.len() { break; }
                let len = read_u32(data, pos + 8, big_endian) as usize;
                (vr, len, 12usize)
            } else {
                let len = read_u16(data, pos + 6, big_endian) as usize;
                (vr, len, 8usize)
            }
        };

        pos += hdr_size;

        // Handle undefined length (0xFFFFFFFF)
        if val_len == 0xFFFFFFFF {
            // Skip this element (sequences etc.), no value to read
            continue;
        }

        // Bounds check
        if pos + val_len > data.len() {
            break;
        }

        let val_data = &data[pos..pos + val_len];
        pos += val_len;

        // Track group 2 end
        if group == 0x0002 {
            if element == 0x0000 && val_len == 4 {
                let group_len = read_u32(val_data, 0, big_endian) as usize;
                // group2end is relative to end of this element
                group2end = Some(pos + group_len);
            }
        }

        // Look up tag info
        let tag_info = lookup_tag(group, element);

        // Determine effective VR: prefer tag table VR for display/formatting
        // For our purposes, use the wire VR if available
        let effective_vr = if vr == b"  " { b"  " } else { vr };

        // Build value
        let name_desc = match tag_info {
            Some((name, desc)) => (name, desc),
            None => continue, // Skip unknown tags
        };

        // Convert value based on VR
        let value_str = build_value_string(val_data, effective_vr, group, element, big_endian);

        // Special handling for TransferSyntaxUID - track for later
        if group == 0x0002 && element == 0x0010 {
            let ts = String::from_utf8_lossy(val_data)
                .trim()
                .trim_end_matches('\0')
                .to_string();
            transfer_syntax = Some(ts);
        }

        // Build the tag with print conversion where applicable
        let print_val = apply_print_conv(group, element, &value_str, val_data, big_endian);

        let tag = Tag {
            id: TagId::Text(name_desc.0.to_string()),
            name: name_desc.0.to_string(),
            description: name_desc.1.to_string(),
            group: TagGroup {
                family0: "DICOM".into(),
                family1: "DICOM".into(),
                family2: "Image".into(),
            },
            raw_value: Value::String(value_str),
            print_value: print_val,
            priority: 0,
        };
        tags.push(tag);
    }

    Ok(tags)
}

fn read_u16(data: &[u8], pos: usize, big_endian: bool) -> u16 {
    if big_endian {
        u16::from_be_bytes([data[pos], data[pos + 1]])
    } else {
        u16::from_le_bytes([data[pos], data[pos + 1]])
    }
}

fn read_u32(data: &[u8], pos: usize, big_endian: bool) -> u32 {
    if big_endian {
        u32::from_be_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]])
    } else {
        u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]])
    }
}

fn build_value_string(val_data: &[u8], vr: &[u8], group: u16, element: u16, big_endian: bool) -> String {
    // Binary pixel data - return description
    if (group == 0x7FE0 && element == 0x0010) || val_data.len() > 1024 {
        return format!("(Binary data {} bytes, use -b option to extract)", val_data.len());
    }

    match vr {
        b"US" => {
            // Unsigned short - may be multiple values
            let mut vals = Vec::new();
            let mut i = 0;
            while i + 2 <= val_data.len() {
                let v = if big_endian {
                    u16::from_be_bytes([val_data[i], val_data[i+1]])
                } else {
                    u16::from_le_bytes([val_data[i], val_data[i+1]])
                };
                vals.push(v.to_string());
                i += 2;
            }
            vals.join(" ")
        }
        b"SS" => {
            let mut vals = Vec::new();
            let mut i = 0;
            while i + 2 <= val_data.len() {
                let v = if big_endian {
                    i16::from_be_bytes([val_data[i], val_data[i+1]])
                } else {
                    i16::from_le_bytes([val_data[i], val_data[i+1]])
                };
                vals.push(v.to_string());
                i += 2;
            }
            vals.join(" ")
        }
        b"UL" => {
            let mut vals = Vec::new();
            let mut i = 0;
            while i + 4 <= val_data.len() {
                let v = read_u32(val_data, i, big_endian);
                vals.push(v.to_string());
                i += 4;
            }
            vals.join(" ")
        }
        b"SL" => {
            let mut vals = Vec::new();
            let mut i = 0;
            while i + 4 <= val_data.len() {
                let v = if big_endian {
                    i32::from_be_bytes([val_data[i], val_data[i+1], val_data[i+2], val_data[i+3]])
                } else {
                    i32::from_le_bytes([val_data[i], val_data[i+1], val_data[i+2], val_data[i+3]])
                };
                vals.push(v.to_string());
                i += 4;
            }
            vals.join(" ")
        }
        b"FL" => {
            let mut vals = Vec::new();
            let mut i = 0;
            while i + 4 <= val_data.len() {
                let bytes = [val_data[i], val_data[i+1], val_data[i+2], val_data[i+3]];
                let v = if big_endian { f32::from_be_bytes(bytes) } else { f32::from_le_bytes(bytes) };
                vals.push(format!("{}", v));
                i += 4;
            }
            vals.join(" ")
        }
        b"FD" => {
            let mut vals = Vec::new();
            let mut i = 0;
            while i + 8 <= val_data.len() {
                let bytes: [u8; 8] = val_data[i..i+8].try_into().unwrap_or([0u8; 8]);
                let v = if big_endian { f64::from_be_bytes(bytes) } else { f64::from_le_bytes(bytes) };
                vals.push(format!("{}", v));
                i += 8;
            }
            vals.join(" ")
        }
        b"OB" | b"OW" => {
            // Binary data
            format!("(Binary data {} bytes, use -b option to extract)", val_data.len())
        }
        b"DA" => {
            // Date: YYYYMMDD
            let s = String::from_utf8_lossy(val_data);
            let s = s.trim().trim_end_matches('\0');
            format_date(s)
        }
        b"TM" => {
            // Time: HHMMSS.FFFFFF
            let s = String::from_utf8_lossy(val_data);
            let s = s.trim().trim_end_matches('\0');
            format_time(s)
        }
        b"UI" => {
            // UID: trim null bytes
            let s = String::from_utf8_lossy(val_data);
            s.trim().trim_end_matches('\0').to_string()
        }
        _ => {
            // String types: trim trailing spaces and nulls
            let s = String::from_utf8_lossy(val_data);
            let s = s.trim_end_matches(' ').trim_end_matches('\0').trim_start_matches(' ');
            // For AE, CS, DS, IS, LO, PN, SH: trim both ends
            s.trim().to_string()
        }
    }
}

fn apply_print_conv(group: u16, element: u16, value_str: &str, val_data: &[u8], big_endian: bool) -> String {
    match (group, element) {
        // PixelRepresentation: 0=Unsigned, 1=Signed
        (0x0028, 0x0103) => {
            if val_data.len() >= 2 {
                let v = if big_endian {
                    u16::from_be_bytes([val_data[0], val_data[1]])
                } else {
                    u16::from_le_bytes([val_data[0], val_data[1]])
                };
                match v {
                    0 => "Unsigned".to_string(),
                    1 => "Signed".to_string(),
                    _ => value_str.to_string(),
                }
            } else {
                value_str.to_string()
            }
        }
        // UI tags: look up UID
        (0x0002, 0x0002) | (0x0002, 0x0003) | (0x0002, 0x0010) | (0x0002, 0x0012)
        | (0x0008, 0x0016) | (0x0008, 0x0018) | (0x0020, 0x000D) | (0x0020, 0x000E)
        | (0x0020, 0x0052) => {
            if let Some(name) = lookup_uid(value_str) {
                name.to_string()
            } else {
                value_str.to_string()
            }
        }
        _ => value_str.to_string(),
    }
}
