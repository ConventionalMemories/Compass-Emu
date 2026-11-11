$COMPACT SS(32)
MODULE crashme;

$NOLIST
$INCLUDE (``INCS`Common.Inc~text~)
$INCLUDE (``INCS`OsPasTypes.Inc~text~)
$INCLUDE (``INCS`OsPasProcs.Inc~text~)
$INCLUDE (``INCS`ConPas.Inc~text~)
$LIST

PUBLIC CrashAsm;
	PROCEDURE DoInt3;
	PROCEDURE DoWild;

PROGRAM crashme;

VAR
	ch: CHAR;
	r, error, k: INTEGER;
	x: REAL;
	msg: ARRAY [0..80] OF BYTE;

PROCEDURE Inner (n: INTEGER);
BEGIN
r := 100 DIV (n - n);
END;

PROCEDURE Middle (n: INTEGER);
BEGIN
Inner (n + 1);
END;

PROCEDURE FloatTest;
VAR j: INTEGER;
BEGIN
x := 1.0;
FOR j := 1 TO 200 DO x := x * 1.01 + 0.5;
END;

FUNCTION Work (n: INTEGER): INTEGER;
VAR j, t: INTEGER;
BEGIN
t := 0;
FOR j := 1 TO 50 DO t := t + (n MOD 7) * j;
Work := t;
END;

PROCEDURE ShowError;
BEGIN
OsDecodeException (2, msg);
END;

BEGIN
ch := ConCharIn;
IF ch = 'd' THEN Middle (5);
IF ch = 'e' THEN ShowError;
IF ch = 'i' THEN DoInt3;
IF ch = 'w' THEN DoWild;
IF ch = 'f' THEN FloatTest;
IF ch = 'p' THEN FOR k := 1 TO 3000 DO r := Work (k);
ch := ConCharIn;
END.
