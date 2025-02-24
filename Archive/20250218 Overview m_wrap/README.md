# Investigate necessary header space for _sparse_m_delta array

Currently, in the midst of specifying requirements in VHF bin files' headers,
namely, with the specific goal of not having to read through the entire file
twice if avoidable.  
Reading the file twice usually occurs in the case where there is a need to
determine the m-offsets midway through the file.
