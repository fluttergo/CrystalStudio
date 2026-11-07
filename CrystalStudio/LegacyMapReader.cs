using System;
using System.IO;

namespace CrystalStudio.Formats
{
    public class CellInfo
    {
        //Back
        public short BackIndex;
        public int BackImage;
        public short TileAnimationImage;
        public short TileAnimationOffset;
        public byte TileAnimationFrames;

        //Middle
        public short MiddleIndex;
        public ushort MiddleImage;
        public byte MiddleAnimationFrame;
        public byte MiddleAnimationTick;

        //Front
        public bool HasFront;
        public short FrontIndex;
        public short FrontImage;
        public byte FrontAnimationFrame;
        public byte FrontAnimationTick;

        //Extra
        public byte DoorIndex;
        public byte DoorOffset;
        public byte Light;
    }

    public class MapReader
    {
        public int Width, Height;
        public CellInfo[,] MapCells = null!;
        public string FileName;
        private byte[] Bytes = [];
        public bool IsCrystalVersion1 => Bytes.Length >= 8 && Bytes[0] == 1 && Bytes[1] == 0 &&
            Bytes[2] == (byte)'C' && Bytes[3] == (byte)'#';
        /// <summary>Full original bytes when the file was decoded by LoadMapType0 (classic Mir2 layout).</summary>
        public byte[]? ClassicImage { get; private set; }

        public MapReader(string FileName)
        {
            this.FileName = FileName;
            initiate();
        }

        private void ValidateDimensions(int headerBytes, int minimumCellBytes)
        {
            long count = (long)Width * Height;
            if (!MapDimensions.CanRoundTrip(Width, Height))
                throw new InvalidDataException("Invalid or excessive map dimensions.");
            if (headerBytes + count * minimumCellBytes > Bytes.LongLength)
                throw new InvalidDataException("Map cell data is truncated.");
        }
        private void initiate()
        {
            Bytes = File.ReadAllBytes(FileName);
            if (Bytes.Length < 52 && !(Bytes.Length >= 8 && Bytes[2] == 0x43 && Bytes[3] == 0x23)) throw new InvalidDataException("Map header is truncated.");
            //c# custom map format
            if ((Bytes[2] == 0x43) && (Bytes[3] == 0x23))
            {
                LoadMapType100();
                return;
            }

            //wemade mir3 maps have no title they just start with blank bytes
            if (Bytes[0] == 0)
            {
                LoadMapType5();
                return;
            }
            //shanda mir3 maps start with title: (C) SNDA, MIR3.
            if ((Bytes[0] == 0x0F) && (Bytes[5] == 0x53) && (Bytes[14] == 0x33))
            {
                LoadMapType6();
                return;
            }
            //wemades antihack map (laby maps) title start with: Mir2 AntiHack
            if ((Bytes[0] == 0x15) && (Bytes[4] == 0x32) && (Bytes[6] == 0x41) && (Bytes[19] == 0x31))
            {
                LoadMapType4();
                return;
            }
            //wemades 2010 map format i guess title starts with: Map 2010 Ver 1.0
            if ((Bytes[0] == 0x10) && (Bytes[2] == 0x61) && (Bytes[7] == 0x31) && (Bytes[14] == 0x31))
            {
                LoadMapType1();
                return;
            }
            //shanda's 2012 format and one of shandas(wemades) older formats share same header info, only difference is the filesize
            if ((Bytes[4] == 0x0F) && (Bytes[18] == 0x0D) && (Bytes[19] == 0x0A))
            {
                int W = Bytes[0] + (Bytes[1] << 8);
                int H = Bytes[2] + (Bytes[3] << 8);
                if (Bytes.Length > 52L + (long)W * H * 14)
                {
                    LoadMapType3();
                    return;
                }
                else
                {
                    LoadMapType2();
                    return;
                }
            }

            //3/4 heroes map format (myth/lifcos i guess)
            if ((Bytes[0] == 0x0D) && (Bytes[1] == 0x4C) && (Bytes[7] == 0x20) && (Bytes[11] == 0x6D))
            {
                LoadMapType7();
                return;
            }

            //if it's none of the above load the default old school format
            ClassicImage = Bytes;
            LoadMapType0();

        }

        private void LoadMapType0()
        {
            try
            {
                int offset = 0;
                Width = BitConverter.ToInt16(Bytes, offset);
                offset += 2;
                Height = BitConverter.ToInt16(Bytes, offset);
                ValidateDimensions(52, 12); MapCells = new CellInfo[Width, Height];
                offset = 52;
                for (int x = 0; x < Width; x++)
                    for (int y = 0; y < Height; y++)
                    {//12
                        MapCells[x, y] = new CellInfo();
                        MapCells[x, y].BackIndex = 0;
                        MapCells[x, y].MiddleIndex = 1;
                        MapCells[x, y].BackImage = (short)BitConverter.ToInt16(Bytes, offset);
                        offset += 2;
                        MapCells[x, y].MiddleImage = (ushort)BitConverter.ToInt16(Bytes, offset);
                        offset += 2;
                        short frimage = (short)BitConverter.ToInt16(Bytes, offset);
                        offset += 2;
                        MapCells[x, y].DoorIndex = Bytes[offset++];
                        MapCells[x, y].DoorOffset = Bytes[offset++];
                        byte franimationframe = Bytes[offset++];
                        byte franimationtick = Bytes[offset++];
                        short frindex = (short)(Bytes[offset++] + 2);
                        MapCells[x, y].Light = Bytes[offset++];
                        if ((MapCells[x, y].BackImage & 0x8000) != 0)
                            MapCells[x, y].BackImage = (MapCells[x, y].BackImage & 0x7FFF) | 0x20000000;


                        if (((frimage & 0x7FFF) > 0) && (frindex > 0))
                        {
                            MapCells[x, y].FrontImage = frimage;
                            MapCells[x, y].FrontIndex = frindex; MapCells[x, y].HasFront = true;
                            MapCells[x, y].FrontAnimationFrame = franimationframe;
                            MapCells[x, y].FrontAnimationTick = franimationtick;

                            if (Convert.ToBoolean(frimage & 0x8000))
                                MapCells[x, y].BackImage |= 0x40000000;
                        }
                    }
            }
            catch (Exception ex)
            {
                throw new InvalidDataException("Invalid or truncated map file.", ex);
            }

        }

        private void LoadMapType1()
        {
            try
            {
                int offSet = 21;

                int w = BitConverter.ToInt16(Bytes, offSet);
                offSet += 2;
                int xor = BitConverter.ToInt16(Bytes, offSet);
                offSet += 2;
                int h = BitConverter.ToInt16(Bytes, offSet);
                Width = w ^ xor;
                Height = h ^ xor;
                ValidateDimensions(54, 15); MapCells = new CellInfo[Width, Height];

                offSet = 54;

                for (int x = 0; x < Width; x++)
                    for (int y = 0; y < Height; y++)
                    {
                        MapCells[x, y] = new CellInfo
                        {
                            BackIndex = 0,
                            BackImage = (int)(BitConverter.ToInt32(Bytes, offSet) ^ 0xAA38AA38),
                            MiddleIndex = 1,
                            MiddleImage = (ushort)(BitConverter.ToUInt16(Bytes, offSet += 4) ^ xor),
                            FrontImage = (short)(BitConverter.ToInt16(Bytes, offSet += 2) ^ xor),
                            DoorIndex = Bytes[offSet += 2],
                            DoorOffset = Bytes[++offSet],
                            FrontAnimationFrame = Bytes[++offSet],
                            FrontAnimationTick = Bytes[++offSet],
                            FrontIndex = (short)(Bytes[++offSet] + 2),
                            HasFront = true,
                            Light = Bytes[++offSet],
                        };
                        offSet += 2; // Skip the unused format byte and advance to the next cell.

                        if (MapCells[x, y].HasFront)
                        {
                            if (Convert.ToBoolean(MapCells[x, y].FrontImage & 0x8000))
                                MapCells[x, y].BackImage |= 0x40000000;
                        }

                    }
            }
            catch (Exception ex)
            {
                throw new InvalidDataException("Invalid or truncated map file.", ex);
            }
        }

        private void LoadMapType2()
        {
            try
            {
                int offset = 0;
                Width = BitConverter.ToInt16(Bytes, offset);
                offset += 2;
                Height = BitConverter.ToInt16(Bytes, offset);
                ValidateDimensions(52, 14); MapCells = new CellInfo[Width, Height];
                offset = 52;
                for (int x = 0; x < Width; x++)
                    for (int y = 0; y < Height; y++)
                    {//14
                        MapCells[x, y] = new CellInfo();
                        MapCells[x, y].BackImage = (short)BitConverter.ToInt16(Bytes, offset);
                        offset += 2;
                        MapCells[x, y].MiddleImage = (ushort)BitConverter.ToUInt16(Bytes, offset);
                        offset += 2;
                        short frimage = (short)BitConverter.ToInt16(Bytes, offset);
                        offset += 2;
                        MapCells[x, y].DoorIndex = Bytes[offset++];
                        MapCells[x, y].DoorOffset = Bytes[offset++];
                        byte franimationframe = Bytes[offset++];
                        byte franimationtick = Bytes[offset++];
                        short frindex = (short)(Bytes[offset++] + 120);
                        MapCells[x, y].Light = Bytes[offset++];
                        MapCells[x, y].BackIndex = (short)(Bytes[offset++] + 100);
                        MapCells[x, y].MiddleIndex = (short)(Bytes[offset++] + 110);
                        if ((MapCells[x, y].BackImage & 0x8000) != 0)
                            MapCells[x, y].BackImage = (MapCells[x, y].BackImage & 0x7FFF) | 0x20000000;


                        if (((frimage & 0x7FFF) > 0) && (frindex > 0))
                        {
                            MapCells[x, y].FrontImage = frimage;
                            MapCells[x, y].FrontIndex = frindex; MapCells[x, y].HasFront = true;
                            MapCells[x, y].FrontAnimationFrame = franimationframe;
                            MapCells[x, y].FrontAnimationTick = franimationtick;

                            if (Convert.ToBoolean(frimage & 0x8000))
                                MapCells[x, y].BackImage |= 0x40000000;
                        }
                    }
            }
            catch (Exception ex)
            {
                throw new InvalidDataException("Invalid or truncated map file.", ex);
            }

        }

        private void LoadMapType3()
        {
            try
            {
                int offset = 0;
                Width = BitConverter.ToInt16(Bytes, offset);
                offset += 2;
                Height = BitConverter.ToInt16(Bytes, offset);
                ValidateDimensions(52, 36); MapCells = new CellInfo[Width, Height];
                offset = 52;
                for (int x = 0; x < Width; x++)
                    for (int y = 0; y < Height; y++)
                    {//36
                        MapCells[x, y] = new CellInfo();
                        MapCells[x, y].BackImage = (short)BitConverter.ToInt16(Bytes, offset);
                        offset += 2;
                        MapCells[x, y].MiddleImage = (ushort)BitConverter.ToUInt16(Bytes, offset);
                        offset += 2;
                        short frimage = (short)BitConverter.ToInt16(Bytes, offset);
                        offset += 2;
                        MapCells[x, y].DoorIndex = Bytes[offset++];
                        MapCells[x, y].DoorOffset = Bytes[offset++];
                        byte franimationframe = Bytes[offset++];
                        byte franimationtick = Bytes[offset++];
                        short frindex = (short)(Bytes[offset++] + 120);
                        MapCells[x, y].Light = Bytes[offset++];
                        MapCells[x, y].BackIndex = (short)(Bytes[offset++] + 100);
                        MapCells[x, y].MiddleIndex = (short)(Bytes[offset++] + 110);
                        MapCells[x, y].TileAnimationImage = (short)BitConverter.ToInt16(Bytes, offset);
                        offset += 7;//2bytes from tileanimframe, 2 bytes always blank?, 2bytes potentialy 'backtiles index', 1byte fileindex for the backtiles?
                        MapCells[x, y].TileAnimationFrames = Bytes[offset++];
                        MapCells[x, y].TileAnimationOffset = (short)BitConverter.ToInt16(Bytes, offset);
                        offset += 14; //tons of light, blending, .. related options i hope
                        if ((MapCells[x, y].BackImage & 0x8000) != 0)
                            MapCells[x, y].BackImage = (MapCells[x, y].BackImage & 0x7FFF) | 0x20000000;


                        if (((frimage & 0x7FFF) > 0) && (frindex > 0))
                        {
                            MapCells[x, y].FrontImage = frimage;
                            MapCells[x, y].FrontIndex = frindex; MapCells[x, y].HasFront = true;
                            MapCells[x, y].FrontAnimationFrame = franimationframe;
                            MapCells[x, y].FrontAnimationTick = franimationtick;

                            if (Convert.ToBoolean(frimage & 0x8000))
                                MapCells[x, y].BackImage |= 0x40000000;
                        }
                    }

            }
            catch (Exception ex)
            {
                throw new InvalidDataException("Invalid or truncated map file.", ex);
            }
        }

        private void LoadMapType4()
        {
            try
            {
                int offset = 31;
                int w = BitConverter.ToInt16(Bytes, offset);
                offset += 2;
                int xor = BitConverter.ToInt16(Bytes, offset);
                offset += 2;
                int h = BitConverter.ToInt16(Bytes, offset);
                Width = w ^ xor;
                Height = h ^ xor;
                ValidateDimensions(64, 12); MapCells = new CellInfo[Width, Height];
                offset = 64;
                for (int x = 0; x < Width; x++)
                    for (int y = 0; y < Height; y++)
                    {//12
                        MapCells[x, y] = new CellInfo();
                        MapCells[x, y].BackIndex = 0;
                        MapCells[x, y].MiddleIndex = 1;
                        MapCells[x, y].BackImage = (short)(BitConverter.ToInt16(Bytes, offset) ^ xor);
                        offset += 2;
                        MapCells[x, y].MiddleImage = (ushort)(BitConverter.ToUInt16(Bytes, offset) ^ xor);
                        offset += 2;
                        short frimage = (short)(BitConverter.ToInt16(Bytes, offset) ^ xor);
                        offset += 2;
                        MapCells[x, y].DoorIndex = Bytes[offset++];
                        MapCells[x, y].DoorOffset = Bytes[offset++];
                        byte franimationframe = Bytes[offset++];
                        byte franimationtick = Bytes[offset++];
                        short frindex = (short)(Bytes[offset++] + 2);
                        MapCells[x, y].Light = Bytes[offset++];
                        if ((MapCells[x, y].BackImage & 0x8000) != 0)
                            MapCells[x, y].BackImage = (MapCells[x, y].BackImage & 0x7FFF) | 0x20000000;


                        if (((frimage & 0x7FFF) > 0) && (frindex > 0))
                        {
                            MapCells[x, y].FrontImage = frimage;
                            MapCells[x, y].FrontIndex = frindex; MapCells[x, y].HasFront = true;
                            MapCells[x, y].FrontAnimationFrame = franimationframe;
                            MapCells[x, y].FrontAnimationTick = franimationtick;

                            if (Convert.ToBoolean(frimage & 0x8000))
                                MapCells[x, y].BackImage |= 0x40000000;
                        }
                    }
            }
            catch (Exception ex)
            {
                throw new InvalidDataException("Invalid or truncated map file.", ex);
            }
        }

        private void LoadMapType5()
        {
            try
            {
                byte flag = 0;
                int offset = 20;
                short Attribute = (short)(BitConverter.ToInt16(Bytes, offset));
                Width = (int)(BitConverter.ToInt16(Bytes, offset += 2));
                Height = (int)(BitConverter.ToInt16(Bytes, offset += 2));
                //ignoring eventfile and fogcolor for now (seems unused in maps i checked)
                offset = 28;
                //initiate all cells
                ValidateDimensions(28 + 3 * ((Width / 2) + (Width % 2)) * (Height / 2), 14); MapCells = new CellInfo[Width, Height];
                for (int x = 0; x < Width; x++)
                    for (int y = 0; y < Height; y++)
                        MapCells[x, y] = new CellInfo();
                //read all back tiles
                for (int x = 0; x < (Width / 2); x++)
                    for (int y = 0; y < (Height / 2); y++)
                    {
                        for (int i = 0; i < 4; i++)
                        {
                            MapCells[(x * 2) + (i % 2), (y * 2) + (i / 2)].BackIndex = (short)(Bytes[offset] != 255 ? Bytes[offset] + 200 : -1);
                            MapCells[(x * 2) + (i % 2), (y * 2) + (i / 2)].BackImage = (int)(BitConverter.ToInt16(Bytes, offset + 1) + 1);
                        }
                        offset += 3;
                    }
                //read rest of data
                offset = 28 + (3 * ((Width / 2) + (Width % 2)) * (Height / 2));
                for (int x = 0; x < Width; x++)
                    for (int y = 0; y < Height; y++)
                    {

                        flag = Bytes[offset++];
                        MapCells[x, y].MiddleAnimationFrame = Bytes[offset++];
                        byte franimationframe = Bytes[offset] == 255 ? (byte)0 : Bytes[offset];
                        franimationframe &= 0x8F;
                        offset++;
                        MapCells[x, y].MiddleAnimationTick = 0;
                        byte franimationtick = 0;
                        short frindex = (short)(Bytes[offset] != 255 ? Bytes[offset] + 200 : -1);
                        offset++;
                        MapCells[x, y].MiddleIndex = (short)(Bytes[offset] != 255 ? Bytes[offset] + 200 : -1);
                        offset++;
                        MapCells[x, y].MiddleImage = (ushort)(BitConverter.ToUInt16(Bytes, offset) + 1);
                        offset += 2;
                        short frimage = (short)(BitConverter.ToInt16(Bytes, offset) + 1);
                        if ((frimage == 1) && (frindex == 200))
                            frindex = -1;
                        offset += 2;
                        offset += 3;//mir3 maps dont have doors so dont bother reading the info
                        MapCells[x, y].Light = (byte)(Bytes[offset] & 0x0F);
                        MapCells[x, y].Light *= 4;//far wants all light on mir3 maps to be maxed :p
                        offset += 2;
                        if ((flag & 0x01) != 1) MapCells[x, y].BackImage |= 0x20000000;
                        if ((flag & 0x02) != 2) MapCells[x, y].BackImage |= 0x40000000;

                        if (((frimage & 0x7FFF) > 0) && (frindex > 0))
                        {
                            MapCells[x, y].FrontIndex = frindex; MapCells[x, y].HasFront = true;
                            MapCells[x, y].FrontImage = frimage;
                            MapCells[x, y].FrontAnimationFrame = franimationframe;
                            MapCells[x, y].FrontAnimationTick = franimationtick;
                        }
                    }
            }
            catch (Exception ex)
            {
                throw new InvalidDataException("Invalid or truncated map file.", ex);
            }
        }

        private void LoadMapType6()
        {
            try
            {
                byte flag = 0;
                int offset = 16;
                Width = BitConverter.ToInt16(Bytes, offset);
                offset += 2;
                Height = BitConverter.ToInt16(Bytes, offset);
                ValidateDimensions(40, 20); MapCells = new CellInfo[Width, Height];
                offset = 40;
                for (int x = 0; x < Width; x++)
                    for (int y = 0; y < Height; y++)
                    {
                        MapCells[x, y] = new CellInfo();
                        flag = Bytes[offset++];
                        MapCells[x, y].BackIndex = (short)(Bytes[offset] != 255 ? Bytes[offset] + 300 : -1);
                        offset++;
                        MapCells[x, y].MiddleIndex = (short)(Bytes[offset] != 255 ? Bytes[offset] + 300 : -1);
                        offset++;
                        short frindex = (short)(Bytes[offset] != 255 ? Bytes[offset] + 300 : -1);
                        offset++;
                        MapCells[x, y].BackImage = (short)(BitConverter.ToInt16(Bytes, offset) + 1);
                        offset += 2;
                        MapCells[x, y].MiddleImage = (ushort)(BitConverter.ToUInt16(Bytes, offset) + 1);
                        offset += 2;
                        short frimage = (short)(BitConverter.ToInt16(Bytes, offset) + 1);
                        offset += 2;
                        if ((frimage == 1) && (frindex == 200))
                            frindex = -1;
                        MapCells[x, y].MiddleAnimationFrame = Bytes[offset++];
                        byte franimationframe = Bytes[offset] == 255 ? (byte)0 : Bytes[offset];
                        if (franimationframe > 0x0F)//assuming shanda used same value not sure
                            franimationframe = (byte)(/*0x80 ^*/ (franimationframe & 0x0F));
                        offset++;
                        MapCells[x, y].MiddleAnimationTick = 1;
                        byte franimationtick = 1;
                        MapCells[x, y].Light = (byte)(Bytes[offset] & 0x0F);
                        MapCells[x, y].Light *= 4;//far wants all light on mir3 maps to be maxed :p
                        offset += 8;
                        if ((flag & 0x01) != 1) MapCells[x, y].BackImage |= 0x20000000;
                        if ((flag & 0x02) != 2) MapCells[x, y].BackImage |= 0x40000000;

                        if (((frimage & 0x7FFF) > 0) && (frindex > 0))
                        {
                            MapCells[x, y].FrontIndex = frindex; MapCells[x, y].HasFront = true;
                            MapCells[x, y].FrontImage = frimage;
                            MapCells[x, y].FrontAnimationFrame = franimationframe;
                            MapCells[x, y].FrontAnimationTick = franimationtick;
                        }
                    }
            }
            catch (Exception ex)
            {
                throw new InvalidDataException("Invalid or truncated map file.", ex);
            }
        }

        private void LoadMapType7()
        {
            try
            {
                int offset = 21;
                Width = BitConverter.ToInt16(Bytes, offset);
                offset += 4;
                Height = BitConverter.ToInt16(Bytes, offset);
                ValidateDimensions(54, 15); MapCells = new CellInfo[Width, Height];

                offset = 54;

                for (int x = 0; x < Width; x++)
                    for (int y = 0; y < Height; y++)
                    {//total 15
                        MapCells[x, y] = new CellInfo
                        {
                            BackIndex = 0,
                            BackImage = (int)BitConverter.ToInt32(Bytes, offset),
                            MiddleIndex = 1,
                            MiddleImage = (ushort)BitConverter.ToUInt16(Bytes, offset += 4),
                            FrontImage = (short)BitConverter.ToInt16(Bytes, offset += 2),
                            DoorIndex = Bytes[offset += 2],
                            DoorOffset = Bytes[++offset],
                            FrontAnimationFrame = Bytes[++offset],
                            FrontAnimationTick = Bytes[++offset],
                            FrontIndex = (short)(Bytes[++offset] + 2),
                            HasFront = true,
                            Light = Bytes[++offset],
                        };
                        offset++; // Skip the unused format byte.
                        if ((MapCells[x, y].BackImage & 0x8000) != 0)
                            MapCells[x, y].BackImage = (MapCells[x, y].BackImage & 0x7FFF) | 0x20000000;

                        if (MapCells[x, y].HasFront)
                        {
                            if (Convert.ToBoolean(MapCells[x, y].FrontImage & 0x8000))
                                MapCells[x, y].BackImage |= 0x40000000;
                        }
                        offset++;

                    }
            }
            catch (Exception ex)
            {
                throw new InvalidDataException("Invalid or truncated map file.", ex);
            }
        }

        private void LoadMapType100()
        {
            try
            {
                int offset = 0;
                var ver = BitConverter.ToInt16(Bytes, offset);
                if (ver < 1 || ver > 5) throw new InvalidDataException("Unsupported Crystal map version.");
                offset += 4;
                Width = BitConverter.ToInt16(Bytes, offset);
                offset += 2;
                Height = BitConverter.ToInt16(Bytes, offset);
                ValidateDimensions(8, ver == 1 ? 26 : ver == 2 ? 38 : ver == 3 ? 35 : 33); MapCells = new CellInfo[Width, Height];
                offset = 8;
                for (int x = 0; x < Width; x++)
                    for (int y = 0; y < Height; y++)
                    {
                        MapCells[x, y] = new CellInfo();
                        MapCells[x, y].BackIndex = (short)BitConverter.ToInt16(Bytes, offset);
                        offset += 2;
                        MapCells[x, y].BackImage = BitConverter.ToInt32(Bytes, offset);
                        offset += 4;
                        MapCells[x, y].MiddleIndex = (short)BitConverter.ToInt16(Bytes, offset);
                        offset += 2;
                        MapCells[x, y].MiddleImage = (ushort)BitConverter.ToUInt16(Bytes, offset);
                        offset += 2;
                        if (ver > 2)
                        {
                            byte frcount = Bytes[offset++];
                            if (frcount > 1) throw new InvalidDataException("A map cell supports one foreground image.");
                            if (frcount == 1)
                            {
                                MapCells[x, y].FrontIndex = (short)BitConverter.ToInt16(Bytes, offset); MapCells[x, y].HasFront = true;
                                offset += 2;
                                MapCells[x, y].FrontImage = (short)BitConverter.ToInt16(Bytes, offset);
                                if (Convert.ToBoolean(MapCells[x, y].FrontImage & 0x8000))
                                    MapCells[x, y].BackImage |= 0x40000000;
                                offset += 2;
                                if (ver > 3)
                                {
                                    MapCells[x, y].FrontAnimationFrame = Bytes[offset++];
                                    MapCells[x, y].FrontAnimationTick = Bytes[offset++];
                                    if (ver > 4) offset++;
                                }
                                else
                                {
                                    MapCells[x, y].FrontAnimationFrame = 0;
                                    MapCells[x, y].FrontAnimationTick = 0;
                                }
                            }
                        }
                        else
                        {
                            short frindex = (short)BitConverter.ToInt16(Bytes, offset);
                            offset += 2;
                            short frimage = (short)BitConverter.ToInt16(Bytes, offset);
                            offset += 2;

                            // Crystal v1 stores these fields verbatim, including library zero.
                            MapCells[x, y].FrontIndex = frindex;
                            MapCells[x, y].FrontImage = frimage;
                            MapCells[x, y].HasFront = frindex != 0 || frimage != 0;
                            if (ver > 1 && (frimage & 0x8000) != 0)
                                MapCells[x, y].BackImage |= 0x40000000;
                        }
                        MapCells[x, y].DoorIndex = (byte)(Bytes[offset++] & 0x7F);
                        MapCells[x, y].DoorOffset = Bytes[offset++];
                        if (ver < 4)
                        {
                            if (MapCells[x, y].HasFront || ver <= 2)
                            {
                                MapCells[x, y].FrontAnimationFrame = Bytes[offset++];
                                MapCells[x, y].FrontAnimationTick = Bytes[offset++];
                                MapCells[x, y].HasFront |= MapCells[x, y].FrontAnimationFrame != 0 || MapCells[x, y].FrontAnimationTick != 0;
                            }
                            else
                                offset += 2;
                        }

                        MapCells[x, y].MiddleAnimationFrame = Bytes[offset++];
                        MapCells[x, y].MiddleAnimationTick = Bytes[offset++];
                        MapCells[x, y].TileAnimationImage = (short)BitConverter.ToInt16(Bytes, offset);
                        offset += 2;
                        MapCells[x, y].TileAnimationOffset = (short)BitConverter.ToInt16(Bytes, offset);
                        offset += 2;
                        MapCells[x, y].TileAnimationFrames = Bytes[offset++];
                        MapCells[x, y].Light = Bytes[offset++];

                        if (ver >= 2)
                        {
                            offset += 8;
                            offset += 2;
                            offset += 2;
                        }

                    }
            }
            catch (Exception ex)
            {
                throw new InvalidDataException("Invalid or truncated map file.", ex);
            }
        }
    }
}




