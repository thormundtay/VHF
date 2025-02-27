# Investigate necessary header space for _sparse_m_delta array

Currently, in the midst of specifying requirements in VHF bin files' headers,
namely, with the specific goal of not having to read through the entire file
twice if avoidable.  
Reading the file twice usually occurs in the case where there is a need to
determine the m-offsets midway through the file.

## Results

We exclude the printing of None length data due to the fact that it is fairly
challenging to go through all of them to see which of the anomalous data sets
is creating such large values. Nonetheless, the data as it is can still be
opened manually as it is a csv. Aternatively, the csv can be viewed in the
command line with
```
  column -t -s ',' --output-width 25 summarized_headers.csv | bat --wrap never
```
where the use of the `head` command and related can help keep the total number
of rows restricted. (The output-width flag might not be the flag we want?.)

Unsurprisingly, files listed as being 20km had the largest mean.
However, some of the 1km data

```
  length         maximum ratio            ratio             std_dev(ratio)
l =   1km, max = 0.000026207035, r = 0.000000150844, e = 1.69743632772515e-06
l =  20km, max = 0.000029440955, r = 0.000005045825, e = 7.1735575390297466e-06
l =  500m, max = 0.000002919202, r = 0.000000417029, e = 1.0215079483756529e-06
l =   0km, max = 0.000040693627, r = 0.000000446586, e = 3.7241364229580563e-06
```

A second round of revision might be necessary to fish out Cintech data.
