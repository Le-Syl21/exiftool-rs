//! IPTC tag definitions.
//!
//! Covers the most common IPTC-IIM Application Record (Record 2) tags.
//! Mirrors ExifTool's IPTC.pm tag tables.

pub struct IptcTagInfo {
    pub record: u8,
    pub dataset: u8,
    pub name: &'static str,
    pub description: &'static str,
    pub is_string: bool,
}

/// Lookup IPTC tag info by record and dataset number.
pub fn lookup(record: u8, dataset: u8) -> Option<&'static IptcTagInfo> {
    IPTC_TAGS
        .iter()
        .find(|t| t.record == record && t.dataset == dataset)
}

/// Check if a tag stores string data.
pub fn is_string_tag(record: u8, dataset: u8) -> bool {
    lookup(record, dataset).map_or(true, |t| t.is_string)
}

static IPTC_TAGS: &[IptcTagInfo] = &[
    // Envelope Record (Record 1)
    IptcTagInfo { record: 1, dataset: 0, name: "EnvelopeRecordVersion", description: "Envelope Record Version", is_string: false },
    IptcTagInfo { record: 1, dataset: 5, name: "Destination", description: "Destination", is_string: true },
    IptcTagInfo { record: 1, dataset: 20, name: "FileFormat", description: "File Format", is_string: false },
    IptcTagInfo { record: 1, dataset: 22, name: "FileVersion", description: "File Format Version", is_string: false },
    IptcTagInfo { record: 1, dataset: 30, name: "ServiceIdentifier", description: "Service Identifier", is_string: true },
    IptcTagInfo { record: 1, dataset: 40, name: "EnvelopeNumber", description: "Envelope Number", is_string: true },
    IptcTagInfo { record: 1, dataset: 60, name: "EnvelopePriority", description: "Envelope Priority", is_string: true },
    IptcTagInfo { record: 1, dataset: 70, name: "DateSent", description: "Date Sent", is_string: true },
    IptcTagInfo { record: 1, dataset: 80, name: "TimeSent", description: "Time Sent", is_string: true },
    IptcTagInfo { record: 1, dataset: 90, name: "CodedCharacterSet", description: "Coded Character Set", is_string: false },
    // Application Record (Record 2) - the main metadata
    IptcTagInfo { record: 2, dataset: 0, name: "ApplicationRecordVersion", description: "Application Record Version", is_string: false },
    IptcTagInfo { record: 2, dataset: 3, name: "ObjectTypeReference", description: "Object Type Reference", is_string: true },
    IptcTagInfo { record: 2, dataset: 4, name: "ObjectAttributeReference", description: "Object Attribute Reference", is_string: true },
    IptcTagInfo { record: 2, dataset: 5, name: "ObjectName", description: "Object Name (Title)", is_string: true },
    IptcTagInfo { record: 2, dataset: 7, name: "EditStatus", description: "Edit Status", is_string: true },
    IptcTagInfo { record: 2, dataset: 10, name: "Urgency", description: "Urgency", is_string: true },
    IptcTagInfo { record: 2, dataset: 12, name: "SubjectReference", description: "Subject Reference", is_string: true },
    IptcTagInfo { record: 2, dataset: 15, name: "Category", description: "Category", is_string: true },
    IptcTagInfo { record: 2, dataset: 20, name: "SupplementalCategories", description: "Supplemental Categories", is_string: true },
    IptcTagInfo { record: 2, dataset: 22, name: "FixtureIdentifier", description: "Fixture Identifier", is_string: true },
    IptcTagInfo { record: 2, dataset: 25, name: "Keywords", description: "Keywords", is_string: true },
    IptcTagInfo { record: 2, dataset: 26, name: "ContentLocationCode", description: "Content Location Code", is_string: true },
    IptcTagInfo { record: 2, dataset: 27, name: "ContentLocationName", description: "Content Location Name", is_string: true },
    IptcTagInfo { record: 2, dataset: 30, name: "ReleaseDate", description: "Release Date", is_string: true },
    IptcTagInfo { record: 2, dataset: 35, name: "ReleaseTime", description: "Release Time", is_string: true },
    IptcTagInfo { record: 2, dataset: 37, name: "ExpirationDate", description: "Expiration Date", is_string: true },
    IptcTagInfo { record: 2, dataset: 38, name: "ExpirationTime", description: "Expiration Time", is_string: true },
    IptcTagInfo { record: 2, dataset: 40, name: "SpecialInstructions", description: "Special Instructions", is_string: true },
    IptcTagInfo { record: 2, dataset: 45, name: "ReferenceService", description: "Reference Service", is_string: true },
    IptcTagInfo { record: 2, dataset: 47, name: "ReferenceDate", description: "Reference Date", is_string: true },
    IptcTagInfo { record: 2, dataset: 50, name: "ReferenceNumber", description: "Reference Number", is_string: true },
    IptcTagInfo { record: 2, dataset: 55, name: "DateCreated", description: "Date Created", is_string: true },
    IptcTagInfo { record: 2, dataset: 60, name: "TimeCreated", description: "Time Created", is_string: true },
    IptcTagInfo { record: 2, dataset: 62, name: "DigitalCreationDate", description: "Digital Creation Date", is_string: true },
    IptcTagInfo { record: 2, dataset: 63, name: "DigitalCreationTime", description: "Digital Creation Time", is_string: true },
    IptcTagInfo { record: 2, dataset: 65, name: "OriginatingProgram", description: "Originating Program", is_string: true },
    IptcTagInfo { record: 2, dataset: 70, name: "ProgramVersion", description: "Program Version", is_string: true },
    IptcTagInfo { record: 2, dataset: 75, name: "ObjectCycle", description: "Object Cycle", is_string: true },
    IptcTagInfo { record: 2, dataset: 80, name: "By-line", description: "By-line (Author)", is_string: true },
    IptcTagInfo { record: 2, dataset: 85, name: "By-lineTitle", description: "By-line Title", is_string: true },
    IptcTagInfo { record: 2, dataset: 90, name: "City", description: "City", is_string: true },
    IptcTagInfo { record: 2, dataset: 92, name: "Sub-location", description: "Sub-location", is_string: true },
    IptcTagInfo { record: 2, dataset: 95, name: "Province-State", description: "Province/State", is_string: true },
    IptcTagInfo { record: 2, dataset: 100, name: "Country-PrimaryLocationCode", description: "Country Code", is_string: true },
    IptcTagInfo { record: 2, dataset: 101, name: "Country-PrimaryLocationName", description: "Country Name", is_string: true },
    IptcTagInfo { record: 2, dataset: 103, name: "OriginalTransmissionReference", description: "Original Transmission Reference", is_string: true },
    IptcTagInfo { record: 2, dataset: 105, name: "Headline", description: "Headline", is_string: true },
    IptcTagInfo { record: 2, dataset: 110, name: "Credit", description: "Credit", is_string: true },
    IptcTagInfo { record: 2, dataset: 115, name: "Source", description: "Source", is_string: true },
    IptcTagInfo { record: 2, dataset: 116, name: "CopyrightNotice", description: "Copyright Notice", is_string: true },
    IptcTagInfo { record: 2, dataset: 118, name: "Contact", description: "Contact", is_string: true },
    IptcTagInfo { record: 2, dataset: 120, name: "Caption-Abstract", description: "Caption/Abstract", is_string: true },
    IptcTagInfo { record: 2, dataset: 122, name: "Writer-Editor", description: "Writer/Editor", is_string: true },
    IptcTagInfo { record: 2, dataset: 130, name: "ImageType", description: "Image Type", is_string: true },
    IptcTagInfo { record: 2, dataset: 131, name: "ImageOrientation", description: "Image Orientation", is_string: true },
    IptcTagInfo { record: 2, dataset: 135, name: "LanguageIdentifier", description: "Language Identifier", is_string: true },
];
