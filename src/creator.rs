use crate::Result;
use crate::{Dictionary, Document, Object, ObjectId};

impl Document {
    /// Create new PDF document with version.
    pub fn with_version<S: Into<String>>(version: S) -> Document {
        let mut document = Self::new();
        document.version = version.into();
        document
    }

    /// Create an object ID.
    pub fn new_object_id(&mut self) -> ObjectId {
        self.max_id += 1;
        (self.max_id, 0)
    }

    /// Add PDF object into document's object list.
    pub fn add_object<T: Into<Object>>(&mut self, object: T) -> ObjectId {
        self.max_id += 1;
        let id = (self.max_id, 0);
        self.objects.insert(id, object.into());
        id
    }

    pub fn set_object<T: Into<Object>>(&mut self, id: ObjectId, object: T) {
        self.objects.insert(id, object.into());
    }

    /// Remove PDF object from document's object list.
    ///
    /// Other objects may still hold references to this object! Therefore, removing the object might
    /// lead to dangling references.
    pub fn remove_object(&mut self, object_id: &ObjectId) -> Result<()> {
        self.objects.remove(object_id);
        Ok(())
    }

    /// Remove annotation from the document.
    ///
    /// References to this annotation are removed from the pages' lists of annotations. Finally, the
    /// annotation object itself is removed.
    pub fn remove_annot(&mut self, object_id: &ObjectId) -> Result<()> {
        for (_, page_id) in self.get_pages() {
            let page = self.get_object_mut(page_id)?.as_dict_mut()?;
            let annots = page.get_mut(b"Annots")?.as_array_mut()?;

            annots.retain(|object| {
                if let Ok(id) = object.as_reference() {
                    return id != *object_id;
                }

                true
            });
        }

        self.remove_object(object_id)?;

        Ok(())
    }

    /// Get the page's resource dictionary.
    ///
    /// Get Object that has the key "Resources".
    pub fn get_or_create_resources(&mut self, page_id: ObjectId) -> Result<&mut Object> {
        let resources_id = {
            let page = self.get_object(page_id).and_then(Object::as_dict)?;
            if page.has(b"Resources") {
                page.get(b"Resources").and_then(Object::as_reference).ok()
            } else {
                None
            }
        };
        if let Some(res_id) = resources_id {
            return self.get_object_mut(res_id);
        }
        let page = self.get_object_mut(page_id).and_then(Object::as_dict_mut)?;
        if !page.has(b"Resources") {
            page.set(b"Resources", Dictionary::new());
        }
        page.get_mut(b"Resources")
    }

    /// Add XObject to a page.
    ///
    /// Get Object that has the key `Resources -> XObject`.
    pub fn add_xobject<N: Into<Vec<u8>>>(
        &mut self, page_id: ObjectId, xobject_name: N, xobject_id: ObjectId,
    ) -> Result<()> {
        if let Ok(resources) = self.get_or_create_resources(page_id).and_then(Object::as_dict_mut) {
            if !resources.has(b"XObject") {
                resources.set("XObject", Dictionary::new());
            }
            let mut xobjects = resources.get_mut(b"XObject")?;
            if let Object::Reference(xobjects_ref_id) = xobjects {
                let mut xobjects_id = *xobjects_ref_id;
                while let Object::Reference(id) = self.get_object(xobjects_id)? {
                    xobjects_id = *id;
                }
                xobjects = self.get_object_mut(xobjects_id)?;
            }
            let xobjects = Object::as_dict_mut(xobjects)?;
            xobjects.set(xobject_name, Object::Reference(xobject_id));
        }
        Ok(())
    }

    /// Add Graphics State to a page.
    ///
    /// Get Object that has the key `Resources -> ExtGState`.
    pub fn add_graphics_state<N: Into<Vec<u8>>>(
        &mut self, page_id: ObjectId, gs_name: N, gs_id: ObjectId,
    ) -> Result<()> {
        if let Ok(resources) = self.get_or_create_resources(page_id).and_then(Object::as_dict_mut) {
            if !resources.has(b"ExtGState") {
                resources.set("ExtGState", Dictionary::new());
            }
            let states = resources.get_mut(b"ExtGState").and_then(Object::as_dict_mut)?;
            states.set(gs_name, Object::Reference(gs_id));
        }
        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use std::path::PathBuf;

    use crate::content::*;
    use crate::{Document, Object, Stream};

    #[cfg(not(feature = "time"))]
    pub fn get_timestamp() -> Object {
        Object::string_literal("D:19700101000000Z")
    }

    #[cfg(feature = "time")]
    pub fn get_timestamp() -> Object {
        time::OffsetDateTime::now_utc().into()
    }

    /// Create and return a document for testing
    pub fn create_document() -> Document {
        create_document_with_texts(&["Hello World!"])
    }

    pub fn create_document_with_texts(texts_for_pages: &[&str]) -> Document {
        let mut doc = Document::with_version("1.5");
        let info_id = doc.add_object(dictionary! {
            "Title" => Object::string_literal("Create PDF document example"),
            "Creator" => Object::string_literal("https://crates.io/crates/lopdf"),
            "CreationDate" => get_timestamp(),
        });
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Courier",
        });
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! {
                "F1" => font_id,
            },
        });
        let contents = texts_for_pages.iter().map(|text| Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 48.into()]),
                Operation::new("Td", vec![100.into(), 600.into()]),
                Operation::new("Tj", vec![Object::string_literal(*text)]),
                Operation::new("ET", vec![]),
            ],
        });

        let pages = contents.map(|content| {
            let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
            let page = doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content_id,
            });
            page.into()
        });

        let pages = dictionary! {
            "Type" => "Pages",
            "Kids" => pages.collect::<Vec<Object>>(),
            "Count" => 1,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        };
        doc.objects.insert(pages_id, Object::Dictionary(pages));
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);
        doc.trailer.set("Info", info_id);
        doc.trailer.set("ID", Object::Array(vec![
            Object::string_literal(b"ABC"),
            Object::string_literal(b"DEF"),
        ]));
        doc.compress();
        doc
    }

    /// Save a document
    pub fn save_document(file_path: &PathBuf, doc: &mut Document) {
        let res = doc.save(file_path);

        assert!(match res {
            Ok(_file) => true,
            Err(_e) => false,
        });
    }

    #[test]
    fn save_created_document() {
        // Create temporary folder to store file.
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test_1_create.pdf");

        let mut doc = create_document();
        // Save to file
        save_document(&file_path, &mut doc);
        assert!(file_path.exists());
    }
}
