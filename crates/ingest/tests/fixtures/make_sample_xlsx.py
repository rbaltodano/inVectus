"""Writes sample.xlsx (two sheets) with the standard library only, for the Excel reader tests.
Run: python3 make_sample_xlsx.py"""
import zipfile, pathlib

strings = ["id", "title", "abstract", "published", "score", "007", "First idea", "Text, with a comma",
           "Second idea", "Line one\nline two", "name", "note", "Other sheet"]
si = "".join(f'<si><t xml:space="preserve">{s.replace("&","&amp;").replace("<","&lt;")}</t></si>' for s in strings)

def s(ref, i): return f'<c r="{ref}" t="s"><v>{i}</v></c>'
def n(ref, v, style=None): return f'<c r="{ref}"{f" s=\"{style}\"" if style is not None else ""}><v>{v}</v></c>'

sheet1 = (
    '<row r="1"/>'  # a blank first row: the header is the first non-empty row
    f'<row r="2">{s("A2",0)}{s("B2",1)}{s("C2",2)}{s("D2",3)}{s("E2",4)}<c r="F2"/></row>'
    f'<row r="3">{s("A3",5)}{s("B3",6)}{s("C3",7)}{n("D3",45658,1)}{n("E3",3)}</row>'
    '<row r="4"/>'
    f'<row r="5">{n("A5",8)}{s("B5",8)}{s("C5",9)}{n("D5",45658.5,2)}{n("E5",2.5)}</row>'
)
sheet2 = f'<row r="1">{s("A1",10)}{s("B1",11)}</row><row r="2">{s("A2",12)}{n("B2",1)}</row>'

def ws(rows): return ('<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">'
                      f'<sheetData>{rows}</sheetData></worksheet>')

files = {
 "[Content_Types].xml": '<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
   '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>'
   '<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>'
   '<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>'
   '<Override PartName="/xl/worksheets/sheet2.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>'
   '<Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>'
   '<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/></Types>',
 "_rels/.rels": '<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
   '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>',
 "xl/workbook.xml": '<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">'
   '<sheets><sheet name="Ideas" sheetId="1" r:id="rId1"/><sheet name="Notes" sheetId="2" r:id="rId2"/></sheets></workbook>',
 "xl/_rels/workbook.xml.rels": '<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
   '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>'
   '<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/>'
   '<Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/>'
   '<Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>',
 "xl/sharedStrings.xml": f'<?xml version="1.0" encoding="UTF-8"?><sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="{len(strings)}" uniqueCount="{len(strings)}">{si}</sst>',
 # Style 1: built-in date format 14; style 2: built-in date-time format 22.
 "xl/styles.xml": '<?xml version="1.0" encoding="UTF-8"?><styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">'
   '<fonts count="1"><font/></fonts><fills count="1"><fill/></fills><borders count="1"><border/></borders>'
   '<cellStyleXfs count="1"><xf/></cellStyleXfs><cellXfs count="3"><xf numFmtId="0"/><xf numFmtId="14" applyNumberFormat="1"/><xf numFmtId="22" applyNumberFormat="1"/></cellXfs></styleSheet>',
 "xl/worksheets/sheet1.xml": ws(sheet1),
 "xl/worksheets/sheet2.xml": ws(sheet2),
}
out = pathlib.Path(__file__).with_name("sample.xlsx")
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
    for name, body in files.items():
        z.writestr(name, body)
print(out)
